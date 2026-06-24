//! A real split: a backend runtime (real store) served over the link wire, and
//! a separate runtime configured `Remote` that forwards a mutation across HTTP
//! into the backend's store.
//!
//! This is the end-to-end capstone of the runtime↔backend link: the production
//! `RemoteTransport` (near node) → `link_router` (far-node HTTP surface) →
//! in-process `Backend` (real `MailService` + store). It proves a mutation
//! forwarded by a remote runtime is applied to the backend's authoritative
//! store — not a mock on either side.
//!
//! The remote **read** path (the runtime serving the backend's data from a
//! replicated base) needs the down-channel to carry served rows; that is the
//! read-replication piece still ahead (L4 §4.3 / W4 coverage). This test covers
//! the write path.
//!
//! @spec docs/replication/L4#3-the-link-contract-backendlink

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use posthaste_authority_runtime::{
    build_authority_runtime, AuthorityRuntimeBuildConfig, BackendTransportConfig, RemoteTransport,
};
use posthaste_domain::{
    AccountDriver, MailboxId, MailboxRecord, MessageId, MessageRecord, MessageSortField, SecretRef,
    SecretStore, SecretStoreError, SortDirection, SyncBatch, SyncCursor, SyncObject, ThreadId,
};
use posthaste_link_contract::{LinkCoverage, LinkTransport};
use posthaste_runtime_contract::{
    AccountTransportMutation, ClientMutationId, CreateAccountMutation, MailListViewState,
    MailPresentationRequest, MailQueryPage, MailQueryRequest, MutationRequest, RuntimeCaller,
    RuntimeCore, SecretWriteMutation, ViewDescriptor,
};
use posthaste_server::link_router;

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

fn build_config(root: PathBuf) -> AuthorityRuntimeBuildConfig {
    AuthorityRuntimeBuildConfig::new(root.join("config"), root.join("state"), root.join("cache"))
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
    let router = link_router(backend.backend_link.transport().clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

// spec: docs/eph/DESIGN-L4-read-replication#6-co-located-is-the-same-code-collapsed
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

    let transport = RemoteTransport::new(base_url);
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
    }
}

// spec: docs/eph/DESIGN-L4-read-replication#2-the-model-one-read-through-cache-policy-per-link
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
    let state: MailListViewState =
        serde_json::from_value(snapshot.data).expect("mail-list state");
    assert!(
        state.rows.iter().any(|row| row.projection["id"] == "m-view"),
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
    let router = link_router(backend.backend_link.transport().clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // A separate runtime configured to reach that backend remotely.
    let remote = build_authority_runtime(
        build_config(temp_root())
            .with_backend_transport(BackendTransportConfig::Remote {
                base_url: format!("http://{addr}"),
            }),
    )
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
