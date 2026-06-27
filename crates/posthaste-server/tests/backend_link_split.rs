//! A real split: a backend runtime (real store) served over the link wire, and
//! a separate runtime configured `Remote` that forwards a mutation across HTTP
//! into the backend's store.
//!
//! This is the end-to-end capstone of the runtime↔backend link: the production
//! `RemoteBackend` (near node) → `link_router` (far-node HTTP surface) →
//! in-process `Backend` (real `MailService` + store). It proves a mutation
//! forwarded by a remote runtime is applied to the backend's authoritative
//! store — not a mock on either side.
//!
//! The remote **read** path (the runtime serving the backend's data from a
//! replicated base) needs the down-channel to carry served rows; that is the
//! read-replication piece still ahead (L4 §4.3 / W4 coverage). This test covers
//! the write path.
//!
//! @spec docs/replication/backend-link/L1#3-the-backendapi-contract

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use posthaste_authority_runtime::{
    build_authority_runtime, build_backend_node, build_remote_runtime, BackendTransportConfig,
    RemoteBackend, RuntimeBuildConfig,
};
use posthaste_domain::{
    AccountDriver, MailboxId, MailboxRecord, MessageId, MessageRecord, MessageSortField, SecretRef,
    SecretStore, SecretStoreError, SetKeywordsCommand, SortDirection, SyncBatch, SyncCursor,
    SyncObject, ThreadId,
};
use posthaste_link_contract::{BackendApi, LinkCoverage};
use posthaste_runtime_contract::{
    AccountTransportMutation, ClientMutationId, CreateAccountMutation, MailListViewState,
    MailPresentationRequest, MailQueryPage, MailQueryRequest, MutationRequest, RuntimeCaller,
    RuntimeCore, SecretWriteMutation, ViewDescriptor,
};
use posthaste_server::{link_router, LinkAuth};

#[derive(Default)]
struct TestSecretStore {
    values: Mutex<HashMap<String, String>>,
}

impl SecretStore for TestSecretStore {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
        self.values
            .lock()
            .unwrap()
            .get(&secret_key(secret_ref))
            .cloned()
            .ok_or_else(|| SecretStoreError::Unavailable("secret not found".to_string()))
    }
    fn save(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.values
            .lock()
            .unwrap()
            .insert(secret_key(secret_ref), value.to_string());
        Ok(())
    }
    fn update(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError> {
        self.save(secret_ref, value)
    }
    fn delete(&self, secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
        self.values.lock().unwrap().remove(&secret_key(secret_ref));
        Ok(())
    }
}

fn secret_key(secret_ref: &SecretRef) -> String {
    format!("{:?}:{}", secret_ref.kind, secret_ref.key)
}

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("ph-link-split-{nanos}-{counter}"));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn account_mutation(id: &str) -> CreateAccountMutation {
    CreateAccountMutation {
        id: Some(id.to_string()),
        name: id.to_string(),
        driver: Some(AccountDriver::Mock),
        enabled: Some(false),
        full_name: None,
        email_patterns: Vec::new(),
        appearance: None,
        transport: AccountTransportMutation::default(),
        secret: SecretWriteMutation::default(),
    }
}

fn build_config(root: PathBuf) -> RuntimeBuildConfig {
    RuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
        .with_secret_store(Arc::new(TestSecretStore::default()))
}

fn seed_inbox_message(
    build: &posthaste_authority_runtime::AuthorityRuntimeBuild,
    account: &posthaste_domain::AccountId,
    message_id: &str,
) {
    build
        .api_bridge
        .store
        .apply_sync_batch(
            account,
            &SyncBatch {
                mailboxes: vec![MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 1,
                }],
                messages: vec![MessageRecord {
                    id: MessageId::from(message_id),
                    source_thread_id: ThreadId::from(format!("thread-{message_id}")),
                    subject: Some("Subject".to_string()),
                    received_at: "2026-06-24T10:00:00Z".to_string(),
                    mailbox_ids: vec![MailboxId::from("inbox")],
                    keywords: vec!["$seen".to_string()],
                    ..Default::default()
                }],
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Message,
                    state: "s1".to_string(),
                    updated_at: "2026-06-24T10:00:00Z".to_string(),
                }],
                ..Default::default()
            },
        )
        .expect("seed applies");
}

async fn serve_link(backend: &posthaste_authority_runtime::AuthorityRuntimeBuild) -> String {
    let router = link_router(backend.backend_link.transport().clone(), LinkAuth::Disabled);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

// spec: docs/replication/backend-link/L3#4-co-located-no-op-short-circuits
#[tokio::test]
async fn remote_transport_reads_a_real_query_over_the_link() {
    // The backend computes the query (the authority owns the query engine); a
    // near node reads through over the link and gets the computed page.
    let backend = build_authority_runtime(build_config(temp_root()))
        .await
        .expect("backend runtime builds");
    let account = backend
        .handle
        .create_account(RuntimeCaller::test(), account_mutation("read-account"))
        .await
        .expect("account creates");
    seed_inbox_message(&backend, &account.id, "m-read");
    let base_url = serve_link(&backend).await;

    let transport = RemoteBackend::new(base_url);
    let page = transport
        .query_mail_page(MailQueryRequest {
            query: format!("in:{}/inbox", account.id.as_str()),
            presentation: MailPresentationRequest::Messages {
                limit: Some(10),
                cursor: None,
                sort_field: MessageSortField::Date,
                sort_direction: SortDirection::Desc,
            },
            visibility: None,
        })
        .await
        .expect("read through the link");

    let MailQueryPage::Messages(page) = page else {
        panic!("expected a message page");
    };
    assert!(
        page.items.iter().any(|m| m.id.as_str() == "m-read"),
        "the far node's computed query should reach the reader over the link"
    );

    // The read channel is distinct from the down-channel; subscribe still works.
    let _ = transport.subscribe(LinkCoverage::Complete).await;
}

fn mail_list_descriptor(query: &str) -> ViewDescriptor {
    let request = MailQueryRequest {
        query: query.to_string(),
        presentation: MailPresentationRequest::Messages {
            limit: Some(10),
            cursor: None,
            sort_field: MessageSortField::Date,
            sort_direction: SortDirection::Desc,
        },
        visibility: None,
    };
    ViewDescriptor {
        family: "mailList".to_string(),
        payload: serde_json::to_value(request).unwrap(),
        ..Default::default()
    }
}

// spec: docs/replication/backend-link/L1#4-reads-are-read-through
#[tokio::test]
async fn remote_runtime_serves_a_mail_list_view_from_the_backend() {
    // The backend holds the data; a Remote runtime holds none of it locally.
    let backend = build_authority_runtime(build_config(temp_root()))
        .await
        .expect("backend runtime builds");
    let account = backend
        .handle
        .create_account(RuntimeCaller::test(), account_mutation("view-account"))
        .await
        .expect("account creates");
    seed_inbox_message(&backend, &account.id, "m-view");
    let base_url = serve_link(&backend).await;

    let remote = build_authority_runtime(build_config(temp_root()).with_backend_transport(
        BackendTransportConfig::Remote {
            base_url: base_url.clone(),
            token: None,
        },
    ))
    .await
    .expect("remote runtime builds");

    // The Remote runtime opens a mail-list view. Its read source reads through
    // the link, so the rows are the backend's computed query — even though the
    // runtime's own store is empty.
    let snapshot = remote
        .handle
        .open_view(
            RuntimeCaller::test(),
            mail_list_descriptor(&format!("in:{}/inbox", account.id.as_str())),
        )
        .await
        .expect("mail-list view opens over the link");
    let state: MailListViewState = serde_json::from_value(snapshot.data).expect("mail-list state");
    assert!(
        state
            .rows
            .iter()
            .any(|row| row.projection["id"] == "m-view"),
        "the split runtime should serve the backend's rows, read through the link"
    );
}

#[tokio::test]
async fn remote_runtime_forwards_a_mutation_into_the_backend_store() {
    // The backend runtime: a real store, one seeded message (unflagged).
    let backend = build_authority_runtime(build_config(temp_root()))
        .await
        .expect("backend runtime builds");
    let account = backend
        .handle
        .create_account(RuntimeCaller::test(), account_mutation("split-account"))
        .await
        .expect("account creates");
    backend
        .api_bridge
        .store
        .apply_sync_batch(
            &account.id,
            &SyncBatch {
                mailboxes: vec![MailboxRecord {
                    id: MailboxId::from("inbox"),
                    name: "Inbox".to_string(),
                    role: Some("inbox".to_string()),
                    unread_emails: 0,
                    total_emails: 1,
                }],
                messages: vec![MessageRecord {
                    id: MessageId::from("m-split"),
                    source_thread_id: ThreadId::from("thread-m-split"),
                    subject: Some("Subject".to_string()),
                    received_at: "2026-06-24T10:00:00Z".to_string(),
                    mailbox_ids: vec![MailboxId::from("inbox")],
                    keywords: vec!["$seen".to_string()],
                    ..Default::default()
                }],
                cursors: vec![SyncCursor {
                    object_type: SyncObject::Message,
                    state: "s1".to_string(),
                    updated_at: "2026-06-24T10:00:00Z".to_string(),
                }],
                ..Default::default()
            },
        )
        .expect("seed applies");

    // Serve the backend's in-process link over the wire.
    let router = link_router(backend.backend_link.transport().clone(), LinkAuth::Disabled);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // A separate runtime configured to reach that backend remotely.
    let remote = build_authority_runtime(build_config(temp_root()).with_backend_transport(
        BackendTransportConfig::Remote {
            base_url: format!("http://{addr}"),
            token: None,
        },
    ))
    .await
    .expect("remote runtime builds");
    let session = remote
        .handle
        .open_session(RuntimeCaller::test())
        .await
        .expect("session opens");

    // Precondition: the backend message exists.
    let before = backend
        .handle
        .get_message_detail(
            RuntimeCaller::test(),
            account.id.clone(),
            MessageId::from("m-split"),
        )
        .await
        .expect("detail read");
    assert!(before.detail.is_some(), "the backend message exists before");

    // The remote runtime forwards a destroy across the HTTP link. (Destroy
    // skips undo-history, so it needs no local read — isolating the write path;
    // the remote runtime holds no replicated copy of this message yet.)
    let receipt = remote
        .handle
        .run_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                session_id: Some(session.session_id.clone()),
                name: "message.destroy".to_string(),
                args: serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "m-split",
                }),
                client_mutation_id: ClientMutationId::new("c-split"),
                context: None,
            },
        )
        .await
        .expect("remote mutation runs");
    assert_eq!(receipt.client_mutation_id.as_str(), "c-split");

    // The backend's authoritative store no longer holds the message.
    let after = backend
        .handle
        .get_message_detail(
            RuntimeCaller::test(),
            account.id.clone(),
            MessageId::from("m-split"),
        )
        .await;
    let gone = match after {
        Ok(result) => result.detail.is_none(),
        Err(_) => true,
    };
    assert!(
        gone,
        "the remote-forwarded destroy should be applied to the backend store"
    );
}

// The macro-generated wire (one method per `for_each_link_op!` row) round-trips
// a read and a write end to end: production `RemoteBackend` (generated client)
// -> `link_router` (generated handler) -> in-process `Backend`.
//
// spec: docs/replication/backend-link/L2#2-backendapi-implementations-localbackend-remotebackend
#[tokio::test]
async fn generated_wire_round_trips_a_read_and_a_write() {
    let backend = build_authority_runtime(build_config(temp_root()))
        .await
        .expect("backend runtime builds");
    let account = backend
        .handle
        .create_account(RuntimeCaller::test(), account_mutation("wire-account"))
        .await
        .expect("account creates");
    seed_inbox_message(&backend, &account.id, "m-wire");
    let base_url = serve_link(&backend).await;

    let transport = RemoteBackend::new(base_url);

    // Generated READ over the wire: the backend's account list reaches the reader.
    let accounts = transport
        .list_accounts()
        .await
        .expect("list_accounts over the link");
    assert!(
        accounts.ids.contains(&account.id),
        "the generated read should see the backend's account"
    );

    // Generated WRITE over the wire: flag the seeded message at the backend.
    transport
        .set_keywords(
            account.id.clone(),
            MessageId::from("m-wire"),
            SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        )
        .await
        .expect("set_keywords over the link");

    // The write hit the authoritative store: a point read reflects it.
    let summary = transport
        .current_summary(account.id.clone(), MessageId::from("m-wire"))
        .await
        .expect("summary over the link")
        .expect("the message is present");
    assert!(
        summary.keywords.iter().any(|keyword| keyword == "$flagged"),
        "the generated write should be applied to the backend store"
    );
}

// The link surface, served with `LinkAuth::Bearer`, rejects requests without a
// matching bearer token and admits those that carry it — the gate a remote mount
// stands behind.
//
// spec: docs/eph/DESIGN-L1-trust-model
#[tokio::test]
async fn link_auth_requires_a_matching_bearer_token() {
    let backend = build_authority_runtime(build_config(temp_root()))
        .await
        .expect("backend runtime builds");
    let account = backend
        .handle
        .create_account(RuntimeCaller::test(), account_mutation("auth-account"))
        .await
        .expect("account creates");

    // Serve the link behind a required bearer token.
    let router = link_router(
        backend.backend_link.transport().clone(),
        LinkAuth::Bearer("s3cret-link-token".to_string()),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let base_url = format!("http://{addr}");

    // No token → rejected.
    let anonymous = RemoteBackend::new(base_url.clone());
    assert!(
        anonymous.list_accounts().await.is_err(),
        "a request with no bearer token must be rejected"
    );

    // Wrong token → rejected.
    let wrong = RemoteBackend::with_token(base_url.clone(), Some("not-the-token".to_string()));
    assert!(
        wrong.list_accounts().await.is_err(),
        "a request with the wrong bearer token must be rejected"
    );

    // Correct token → authorized, and the generated read reaches the backend.
    let authed = RemoteBackend::with_token(base_url, Some("s3cret-link-token".to_string()));
    let accounts = authed
        .list_accounts()
        .await
        .expect("a request with the correct bearer token is authorized");
    assert!(
        accounts.ids.contains(&account.id),
        "the authenticated read should see the backend's account"
    );
}

// The `posthaste-backend` role: a STANDALONE backend far node (build_backend_node,
// no runtime near node) served over the link drives reads AND writes — incl.
// account CRUD — for a remote runtime.
//
// spec: docs/replication/backend-link/L2#7-the-build-seam-and-role-binaries
#[tokio::test]
async fn standalone_backend_node_serves_the_link() {
    let node = build_backend_node(build_config(temp_root()))
        .await
        .expect("backend node builds standalone");

    let router = link_router(node.transport(), LinkAuth::Disabled);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let transport = RemoteBackend::new(format!("http://{addr}"));

    // Write over the link: create an account at the standalone backend.
    let created = transport
        .create_account(account_mutation("standalone-account"))
        .await
        .expect("create_account over the link");
    assert_eq!(created.id.as_str(), "standalone-account");

    // Read it back over the link.
    let accounts = transport
        .list_accounts()
        .await
        .expect("list_accounts over the link");
    assert!(
        accounts.ids.contains(&created.id),
        "the standalone backend should serve the account it just created"
    );
}

// The `posthaste-runtime` role: a LEAN near node (build_remote_runtime, NO local
// backend — no store/service/supervisor) drives a standalone backend entirely
// over the link, through the normal RuntimeCore handle clients use.
//
// spec: docs/replication/backend-link/L2#7-the-build-seam-and-role-binaries
#[tokio::test]
async fn lean_remote_runtime_drives_the_backend_over_the_link() {
    // A standalone backend served over the link.
    let backend = build_backend_node(build_config(temp_root()))
        .await
        .expect("backend node builds");
    let router = link_router(backend.transport(), LinkAuth::Disabled);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // A backend-less runtime pointed at it.
    let runtime = build_remote_runtime(build_config(temp_root()).with_backend_transport(
        BackendTransportConfig::Remote {
            base_url: format!("http://{addr}"),
            token: None,
        },
    ))
    .expect("lean remote runtime builds");

    // Drive it through the normal client-facing handle: a write, then a read,
    // both crossing the link to the backend (the runtime holds no store).
    let created = runtime
        .handle
        .create_account(RuntimeCaller::test(), account_mutation("lean-account"))
        .await
        .expect("create_account through the lean runtime");
    assert_eq!(created.id.as_str(), "lean-account");

    let accounts = runtime
        .handle
        .list_accounts(RuntimeCaller::test())
        .await
        .expect("list_accounts through the lean runtime");
    assert!(
        accounts.ids.contains(&created.id),
        "the lean runtime should serve the account it created over the link"
    );
}
