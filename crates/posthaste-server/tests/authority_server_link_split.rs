//! A real split: an authority server runtime (real store) served over the link wire, and
//! a separate runtime configured `Remote` that forwards a mutation across HTTP
//! into the authority server's store.
//!
//! This is the end-to-end capstone of the runtime↔authority-server link: the production
//! `RemoteAuthorityServer` (near node) → `link_router` (far-node HTTP surface) →
//! in-process `AuthorityServer` (real `MailService` + store). It proves a mutation
//! forwarded by a remote runtime is applied to the authority server's authoritative
//! store — not a mock on either side.
//!
//! The remote **read** path (the runtime serving the authority server's data from a
//! replicated base) needs the down-channel to carry served rows; that is the
//! read-replication piece still ahead (L4 §4.3 / W4 coverage). This test covers
//! the write path.
//!
//! @spec docs/replication/authority-server-link/L1#3-the-backendapi-contract

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use posthaste_authority_server::{build_authority_server, build_authority_server_node};
use posthaste_domain_model::{
    AccountDriver, MailboxId, MailboxRecord, MessageId, MessageRecord, MessageSortField, SecretRef,
    SecretStoreError, SetKeywordsCommand, SortDirection, SyncBatch, SyncCursor, SyncObject,
    ThreadId,
};
use posthaste_domain_service::SecretStore;
use posthaste_authority_server_link::{
    AuthorityServerApi, AuthorityServerFrame, AuthorityServerLink, AuthorityServerLinkId,
    LinkCoverage,
};
use posthaste_client_link::RuntimeLink;
use posthaste_contract_core::mutation_args::MessageSetKeywordsMutationArgs;
use posthaste_contract_core::{
    AccountTransportMutation, ClientMutationId, CreateAccountMutation, MailListViewState,
    MailOperation, MailPresentationRequest, MailQueryPage, MailQueryRequest, MutationRequest,
    RuntimeCaller, SecretWriteMutation, ViewDescriptor,
};
use posthaste_runtime::{build_remote_runtime, AuthorityServerTransportConfig, RemoteAuthorityServer, RuntimeBuildConfig};
use posthaste_runtime_api::{RuntimeAccountApi, RuntimeMailReadApi};
use posthaste_authority_server::{link_router, LinkAuth};

use futures_util::StreamExt;

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
        signature: None,
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
    build: &posthaste_authority_server::AuthorityServerBuild,
    account: &posthaste_domain_model::AccountId,
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

async fn serve_link(authority_server: &posthaste_authority_server::AuthorityServerBuild) -> String {
    let router = link_router(authority_server.authority_server_link.clone(), LinkAuth::Disabled);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

// spec: docs/replication/authority-server-link/L3#4-co-located-no-op-short-circuits
#[tokio::test]
async fn remote_transport_reads_a_real_query_over_the_link() {
    // The authority server computes the query (the authority owns the query engine); a
    // near node reads through over the link and gets the computed page.
    let authority_server = build_authority_server(build_config(temp_root()))
        .await
        .expect("authority_server runtime builds");
    let account = authority_server
        .handle
        .create_account(RuntimeCaller::test(), account_mutation("read-account"))
        .await
        .expect("account creates");
    seed_inbox_message(&authority_server, &account.id, "m-read");
    let base_url = serve_link(&authority_server).await;

    let transport = RemoteAuthorityServer::new(base_url);
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

// spec: docs/replication/authority-server-link/L1#4-reads-are-read-through
#[tokio::test]
async fn remote_runtime_serves_a_mail_list_view_from_the_authority_server() {
    // The authority server holds the data; a Remote runtime holds none of it locally.
    let authority_server = build_authority_server(build_config(temp_root()))
        .await
        .expect("authority_server runtime builds");
    let account = authority_server
        .handle
        .create_account(RuntimeCaller::test(), account_mutation("view-account"))
        .await
        .expect("account creates");
    seed_inbox_message(&authority_server, &account.id, "m-view");
    let base_url = serve_link(&authority_server).await;

    let remote = build_authority_server(build_config(temp_root()).with_authority_server_transport(
        AuthorityServerTransportConfig::Remote {
            base_url: base_url.clone(),
            token: None,
        },
    ))
    .await
    .expect("remote runtime builds");

    // The Remote runtime opens a mail-list view. Its read source reads through
    // the link, so the rows are the authority server's computed query — even though the
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
        "the split runtime should serve the authority server's rows, read through the link"
    );
}

#[tokio::test]
async fn remote_runtime_forwards_a_mutation_into_the_authority_server_store() {
    // The authority server runtime: a real store, one seeded message (unflagged).
    let authority_server = build_authority_server(build_config(temp_root()))
        .await
        .expect("authority_server runtime builds");
    let account = authority_server
        .handle
        .create_account(RuntimeCaller::test(), account_mutation("split-account"))
        .await
        .expect("account creates");
    authority_server
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

    // Serve the authority server's in-process link over the wire.
    let router = link_router(authority_server.authority_server_link.clone(), LinkAuth::Disabled);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // A separate runtime configured to reach that authority server remotely.
    let remote = build_authority_server(build_config(temp_root()).with_authority_server_transport(
        AuthorityServerTransportConfig::Remote {
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

    // Precondition: the authority server message exists.
    let before = authority_server
        .handle
        .get_message_detail(
            RuntimeCaller::test(),
            account.id.clone(),
            MessageId::from("m-split"),
        )
        .await
        .expect("detail read");
    assert!(before.detail.is_some(), "the authority server message exists before");

    // The remote runtime forwards a destroy across the HTTP link. (Destroy
    // skips undo-history, so it needs no local read — isolating the write path;
    // the remote runtime holds no replicated copy of this message yet.)
    let receipt = remote
        .handle
        .forward_mutation(
            RuntimeCaller::test(),
            MutationRequest {
                session_id: Some(session.session_id.clone()),
                operation: serde_json::from_value(serde_json::json!({
                    "name": "message.destroy",
                    "args": serde_json::json!({
                    "sourceId": account.id.as_str(),
                    "messageId": "m-split",
                }),
                }))
                .expect("typed operation parses"),
                client_mutation_id: ClientMutationId::new("c-split"),
                context: None,
            },
        )
        .await
        .expect("remote mutation runs");
    assert_eq!(receipt.client_mutation_id.as_str(), "c-split");

    // The authority server's authoritative store no longer holds the message.
    let after = authority_server
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
        "the remote-forwarded destroy should be applied to the authority server store"
    );
}

// The macro-generated wire (one method per `for_each_link_api_op!` row) plus
// the M5b direct-apply command bridge round-trip a read and a write end to end:
// production `RemoteAuthorityServer` (generated client + `apply` dispatch)
// -> `link_router` (generated handler / preserved per-command route)
// -> in-process `AuthorityServer`.
//
// spec: docs/replication/authority-server-link/L2#2-backendapi-implementations-localbackend-remotebackend
#[tokio::test]
async fn generated_wire_round_trips_a_read_and_a_write() {
    let authority_server = build_authority_server(build_config(temp_root()))
        .await
        .expect("authority_server runtime builds");
    let account = authority_server
        .handle
        .create_account(RuntimeCaller::test(), account_mutation("wire-account"))
        .await
        .expect("account creates");
    seed_inbox_message(&authority_server, &account.id, "m-wire");
    let base_url = serve_link(&authority_server).await;

    let transport = RemoteAuthorityServer::new(base_url);

    // Generated READ over the wire: the authority server's account list reaches the reader.
    let accounts = transport
        .list_accounts()
        .await
        .expect("list_accounts over the link");
    assert!(
        accounts.ids.contains(&account.id),
        "the generated read should see the authority server's account"
    );

    // Typed WRITE over the wire: flag the seeded message at the authority
    // server through the single direct-apply entry (M5b — `apply(op)` rides the
    // preserved per-command route).
    transport
        .apply(MailOperation::SetKeywords(MessageSetKeywordsMutationArgs {
            source_id: account.id.as_str().to_string(),
            message_id: "m-wire".to_string(),
            command: SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            },
        }))
        .await
        .expect("apply(set-keywords) over the link");

    // The write hit the authoritative store: a point read reflects it.
    let summary = transport
        .current_summary(account.id.clone(), MessageId::from("m-wire"))
        .await
        .expect("summary over the link")
        .expect("the message is present");
    assert!(
        summary.keywords.iter().any(|keyword| keyword == "$flagged"),
        "the generated write should be applied to the authority server store"
    );
}

// The link surface, served with `LinkAuth::PerRuntime`, rejects requests without a
// matching bearer token and admits those that carry it — the gate a remote mount
// stands behind.
//
// spec: docs/eph/DESIGN-L1-trust-model
#[tokio::test]
async fn link_auth_requires_a_matching_bearer_token() {
    let authority_server = build_authority_server(build_config(temp_root()))
        .await
        .expect("authority_server runtime builds");
    let account = authority_server
        .handle
        .create_account(RuntimeCaller::test(), account_mutation("auth-account"))
        .await
        .expect("account creates");

    // Serve the link behind a required bearer token.
    let router = link_router(
        authority_server.authority_server_link.clone(),
        LinkAuth::PerRuntime(HashMap::from([(
            "s3cret-link-token".to_string(),
            AuthorityServerLinkId::new("rt-test"),
        )])),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let base_url = format!("http://{addr}");

    // No token → rejected.
    let anonymous = RemoteAuthorityServer::new(base_url.clone());
    assert!(
        anonymous.list_accounts().await.is_err(),
        "a request with no bearer token must be rejected"
    );

    // Wrong token → rejected.
    let wrong = RemoteAuthorityServer::with_token(base_url.clone(), Some("not-the-token".to_string()));
    assert!(
        wrong.list_accounts().await.is_err(),
        "a request with the wrong bearer token must be rejected"
    );

    // Correct token → authorized, and the generated read reaches the authority server.
    let authed = RemoteAuthorityServer::with_token(base_url, Some("s3cret-link-token".to_string()));
    let accounts = authed
        .list_accounts()
        .await
        .expect("a request with the correct bearer token is authorized");
    assert!(
        accounts.ids.contains(&account.id),
        "the authenticated read should see the authority server's account"
    );
}

// The `posthaste-authority-server` role: a STANDALONE authority server far node (build_authority_server_node,
// no runtime near node) served over the link drives reads AND writes — incl.
// account CRUD — for a remote runtime.
//
// spec: docs/replication/authority-server-link/L2#7-the-build-seam-and-role-binaries
#[tokio::test]
async fn standalone_authority_server_node_serves_the_link() {
    let node = build_authority_server_node(build_config(temp_root()))
        .await
        .expect("authority_server node builds standalone");

    let router = link_router(node.transport(), LinkAuth::Disabled);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let transport = RemoteAuthorityServer::new(format!("http://{addr}"));

    // Write over the link: create an account at the standalone authority server.
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
        "the standalone authority server should serve the account it just created"
    );
}

// The `posthaste-runtime` role: a LEAN near node (build_remote_runtime, NO local
// authority server — no store/service/supervisor) drives a standalone authority server entirely
// over the link, through the normal RuntimeCore handle clients use.
//
// spec: docs/replication/authority-server-link/L2#7-the-build-seam-and-role-binaries
#[tokio::test]
async fn lean_remote_runtime_drives_the_authority_server_over_the_link() {
    // A standalone authority server served over the link.
    let authority_server = build_authority_server_node(build_config(temp_root()))
        .await
        .expect("authority_server node builds");
    let router = link_router(authority_server.transport(), LinkAuth::Disabled);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // An authority-server-less runtime pointed at it.
    let runtime = build_remote_runtime(build_config(temp_root()).with_authority_server_transport(
        AuthorityServerTransportConfig::Remote {
            base_url: format!("http://{addr}"),
            token: None,
        },
    ))
    .expect("lean remote runtime builds");

    // Drive it through the normal client-facing handle: a write, then a read,
    // both crossing the link to the authority server (the runtime holds no store).
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

// A forwarded mutation's per-mutation confirmation (`AuthorityServerFrame::Settlement`) is
// routed onto the originating runtime's down-stream only — the load-bearing
// multi-runtime invariant (`settlement-routed-to-origin-runtime`). Proved
// end-to-end over the real wire: a `RemoteAuthorityServer` subscribes, forwards a
// mutation, and its down-stream delivers a Settlement naming it.
#[tokio::test]
async fn a_forwarded_mutation_settles_onto_the_originating_runtimes_down_stream() {
    let authority_server = build_authority_server(build_config(temp_root()))
        .await
        .expect("authority_server runtime builds");
    let account = authority_server
        .handle
        .create_account(RuntimeCaller::test(), account_mutation("settle-account"))
        .await
        .expect("account creates");
    authority_server
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
                    id: MessageId::from("m-settle"),
                    source_thread_id: ThreadId::from("thread-m-settle"),
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

    // Serve the link behind per-runtime auth: one runtime, "rt-1", token "t1".
    let router = link_router(
        authority_server.authority_server_link.clone(),
        LinkAuth::PerRuntime(HashMap::from([(
            "t1".to_string(),
            AuthorityServerLinkId::new("rt-1"),
        )])),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let transport =
        RemoteAuthorityServer::with_token(format!("http://{addr}"), Some("t1".to_string()));
    // Subscribe first so the settlement sink exists, then forward.
    let mut down = transport
        .subscribe(LinkCoverage::Complete)
        .await
        .expect("subscribe over the wire");
    transport
        .forward_mutation(MutationRequest {
            session_id: None,
            operation: serde_json::from_value(serde_json::json!({
                "name": "message.destroy",
                "args": serde_json::json!({
                "sourceId": account.id.as_str(),
                "messageId": "m-settle",
            }),
            }))
            .expect("typed operation parses"),
            client_mutation_id: ClientMutationId::new("c-settle"),
            context: None,
        })
        .await
        .expect("forward over the wire");

    // The down-stream must deliver a Settlement for this mutation (the message
    // update Base may arrive first — skip until the Settlement is seen).
    let mut saw_settlement = false;
    for _ in 0..64 {
        match tokio::time::timeout(Duration::from_secs(2), down.next()).await {
            Ok(Some(AuthorityServerFrame::Settlement { .. })) => {
                saw_settlement = true;
                break;
            }
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    assert!(
        saw_settlement,
        "the originating runtime's down-stream must receive a AuthorityServerFrame::Settlement"
    );
}
