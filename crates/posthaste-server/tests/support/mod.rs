//! Full-stack integration harness: a real [`AppState`] (real `DatabaseStore` +
//! `MailService` + on-disk config) wired into the REAL `/v1` router via
//! [`posthaste_server::build_api_router`]. Tests drive the actual handlers
//! through the actual `require_auth` perimeter — no stub routes — so handler
//! result-side scoping and end-to-end token behavior are exercised against real
//! seeded data.
//!
//! Seed via [`Harness::seed_source`] + [`Harness::seed_messages`] (the public
//! store API), mint tokens with [`Harness::full_scope`] / [`Harness::scoped`],
//! and drive requests with [`Harness::get_json`].

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, AppSettings,
    ConfigRepository, MailService, MailStore, MailboxId, MailboxRecord, MessageId, MessageRecord,
    Recipient, SecretRef, SecretStore, SecretStoreError, SourceProjectionStore, SyncBatch,
    SyncCursor, SyncObject, SyncWriteStore, ThreadId, RFC3339_EPOCH,
};
use posthaste_runtime_contract::{RuntimeAccountList, RuntimeCaller, RuntimeCore, RuntimeStatus};
use posthaste_server::supervisor::AccountSupervisor;
use posthaste_server::token::{attenuate, mint_full_scope_token, mint_with_caveats, RootKey};
use posthaste_server::{build_api_router, AppState};
use posthaste_store::DatabaseStore;
use tokio::sync::broadcast;

const CORS_ORIGIN: &str = "http://localhost:5173";

/// A deterministic 32-byte root key so minted/attenuated tokens verify.
fn test_root_key() -> RootKey {
    RootKey::from_test_bytes([42u8; 32])
}

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("posthaste-full-stack-test-{now}-{seq}"))
}

/// A no-op secret store; the full-stack reads exercised here need no secrets.
struct TestSecretStore;

impl SecretStore for TestSecretStore {
    fn resolve(&self, _secret_ref: &SecretRef) -> Result<String, SecretStoreError> {
        Err(SecretStoreError::Unavailable("unused".to_string()))
    }
    fn save(&self, _secret_ref: &SecretRef, _value: &str) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::Unsupported("unused".to_string()))
    }
    fn update(&self, _secret_ref: &SecretRef, _value: &str) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::Unsupported("unused".to_string()))
    }
    fn delete(&self, _secret_ref: &SecretRef) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::Unsupported("unused".to_string()))
    }
}

/// A full-stack test server: real state + real `/v1` router.
pub struct Harness {
    db: Arc<DatabaseStore>,
    router: Router,
    root: RootKey,
    state: Arc<AppState>,
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    supervisor: Arc<AccountSupervisor>,
}

impl Harness {
    /// Build a fresh harness with `require_auth` ON and an empty store.
    pub fn new() -> Self {
        let root_dir = temp_root();
        let config_root = root_dir.join("config");
        let state_root = root_dir.join("state");
        let config_repo =
            TomlConfigRepository::open(&config_root).expect("config repository should open");
        config_repo
            .initialize_defaults()
            .expect("config defaults should initialize");
        let db = Arc::new(
            DatabaseStore::open(state_root.join("mail.sqlite"), &state_root)
                .expect("database store should open"),
        );
        let store: Arc<dyn MailStore> = db.clone();
        let config: Arc<dyn ConfigRepository> = Arc::new(config_repo);
        let service = Arc::new(MailService::new(db.clone(), config));
        let (event_sender, _) = broadcast::channel(16);
        let secret_store: Arc<dyn SecretStore> = Arc::new(TestSecretStore);
        let supervisor = Arc::new(AccountSupervisor::new(
            service.clone(),
            store.clone(),
            secret_store.clone(),
            event_sender.clone(),
            Duration::from_secs(60),
        ));
        let root = test_root_key();
        let state = Arc::new(AppState {
            runtime: posthaste_server::runtime_handle_with_account_runtime_provider_for_migration(
                service.clone(),
                store.clone(),
                secret_store.clone(),
                event_sender,
                supervisor.clone(),
            ),
            account_logo_root: state_root.join("account-assets/logos"),
            auth_token: mint_full_scope_token(&root),
            macaroon_root_key: root.clone(),
            require_auth: true,
            origin_allowlist: posthaste_server::auth::origin_allowlist(CORS_ORIGIN, &[]),
            host_allowlist: posthaste_server::auth::host_allowlist("127.0.0.1:3001"),
        });
        let router = Router::new().nest("/v1", build_api_router(state.clone()));
        Self {
            db,
            router,
            root,
            state,
            service,
            store,
            supervisor,
        }
    }

    /// Read runtime status through the API adapter state wrapper.
    pub async fn runtime_status(&self) -> RuntimeStatus {
        self.state
            .runtime
            .runtime_status(RuntimeCaller::test())
            .await
            .expect("runtime status should be readable")
    }

    /// Read account list through the runtime handle in API adapter state.
    pub async fn runtime_accounts(&self) -> RuntimeAccountList {
        self.state
            .runtime
            .list_accounts(RuntimeCaller::test())
            .await
            .expect("runtime accounts should be readable")
    }

    /// Read app settings through the runtime handle in API adapter state.
    pub async fn runtime_app_settings(&self) -> AppSettings {
        self.state
            .runtime
            .get_app_settings(RuntimeCaller::test())
            .await
            .expect("runtime app settings should be readable")
    }

    /// Persist a configured account through the service.
    pub fn remember_sender_address(&self, account: &str, name: Option<&str>, email: &str) {
        self.store
            .remember_sender_address(
                &AccountId::from(account),
                &Recipient {
                    name: name.map(str::to_string),
                    email: email.to_string(),
                },
            )
            .expect("sender address should save");
    }

    pub fn save_account(&self, id: &str, name: &str, enabled: bool) {
        self.service
            .save_source(&AccountSettings {
                id: AccountId::from(id),
                name: name.to_string(),
                full_name: None,
                signature: None,
                email_patterns: Vec::new(),
                driver: AccountDriver::Mock,
                enabled,
                appearance: None,
                transport: AccountTransportSettings::default(),
                created_at: RFC3339_EPOCH.to_string(),
                updated_at: RFC3339_EPOCH.to_string(),
            })
            .expect("account should save");
    }

    pub async fn start_account_runtime(&self, id: &str) {
        let account = self
            .service
            .get_source(&AccountId::from(id))
            .expect("account lookup should succeed")
            .expect("account should exist");
        self.supervisor.start_account(&account).await;
        self.supervisor
            .sync_account(&account.id)
            .await
            .expect("mock account runtime should sync");
    }

    /// Register a source (account id → display name) so conversation/message
    /// joins resolve. Call before seeding messages for that account.
    pub fn seed_source(&self, account: &str, name: &str) {
        self.db
            .upsert_source_projection(&AccountId::from(account), name)
            .expect("seed source projection");
    }

    /// Apply a sync batch of `messages` (in `mailbox`) to `account`.
    pub fn seed_messages(&self, account: &str, mailbox: &str, messages: Vec<MessageRecord>) {
        self.db
            .apply_sync_batch(
                &AccountId::from(account),
                &SyncBatch {
                    mailboxes: vec![MailboxRecord {
                        id: MailboxId::from(mailbox),
                        name: "Inbox".to_string(),
                        role: Some("inbox".to_string()),
                        unread_emails: 0,
                        total_emails: 0,
                    }],
                    messages,
                    imap_mailbox_states: Vec::new(),
                    imap_message_locations: Vec::new(),
                    deleted_imap_message_locations: Vec::new(),
                    deleted_mailbox_ids: Vec::new(),
                    deleted_message_ids: Vec::new(),
                    replace_all_mailboxes: false,
                    replace_all_messages: false,
                    cursors: vec![SyncCursor {
                        object_type: SyncObject::Message,
                        state: format!("{account}-state"),
                        updated_at: "2026-03-31T10:00:00Z".to_string(),
                    }],
                },
            )
            .expect("seed messages");
    }

    /// A full-scope token (no caveats) for this harness's root key.
    pub fn full_scope(&self) -> String {
        mint_full_scope_token(&self.root)
    }

    /// A token carrying the given caveat predicates (e.g. `"action = read"`).
    pub fn scoped(&self, predicates: &[&str]) -> String {
        mint_with_caveats(&self.root, predicates)
    }

    /// Attenuate an existing token with one more predicate.
    pub fn attenuate(&self, token: &str, predicate: &str) -> String {
        attenuate(token, predicate).expect("attenuate should succeed")
    }

    /// `GET path` with a bearer token and an allowlisted Host; returns the
    /// status and parsed JSON body (`Null` when empty/unparseable).
    pub async fn get_json(&self, token: &str, path: &str) -> (StatusCode, serde_json::Value) {
        self.request_json(Method::GET, token, path, None).await
    }

    /// `POST path` with a bearer token, JSON body, and allowlisted Host.
    pub async fn post_json(
        &self,
        token: &str,
        path: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        self.request_json(Method::POST, token, path, Some(body))
            .await
    }

    /// `PATCH path` with a bearer token, JSON body, and allowlisted Host.
    pub async fn patch_json(
        &self,
        token: &str,
        path: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        self.request_json(Method::PATCH, token, path, Some(body))
            .await
    }

    /// `DELETE path` with a bearer token and allowlisted Host.
    pub async fn delete_json(&self, token: &str, path: &str) -> (StatusCode, serde_json::Value) {
        self.request_json(Method::DELETE, token, path, None).await
    }

    /// `GET path` and return the first response body data frame. Useful for
    /// infinite SSE bodies where collecting the full body would hang.
    /// Open an SSE endpoint and return `(status, content-type)` WITHOUT draining
    /// the (potentially infinite) event body. Proves the handler opened the
    /// stream against the real runtime; the body is dropped unread.
    pub async fn sse_open(&self, token: &str, path: &str) -> (StatusCode, Option<String>) {
        let request = Request::builder()
            .method(Method::GET)
            .uri(path)
            .header(header::HOST, "127.0.0.1")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .expect("request should build");
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("router should respond");
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        (status, content_type)
    }

    pub async fn get_text_frame(&self, token: &str, path: &str) -> (StatusCode, String) {
        let request = Request::builder()
            .method(Method::GET)
            .uri(path)
            .header(header::HOST, "127.0.0.1")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .expect("request should build");
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("router should respond");
        let status = response.status();
        let mut body = response.into_body();
        let frame = tokio::time::timeout(Duration::from_secs(1), body.frame())
            .await
            .expect("body frame should arrive")
            .expect("body should not end before a frame")
            .expect("body frame should succeed");
        let bytes = frame.into_data().expect("first frame should be data");
        let text = String::from_utf8(bytes.to_vec()).expect("frame should be utf-8");
        (status, text)
    }

    async fn request_json(
        &self,
        method: Method,
        token: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let body = body
            .map(|body| Body::from(serde_json::to_vec(&body).expect("body should serialize")))
            .unwrap_or_else(Body::empty);
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, "127.0.0.1")
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .expect("request should build");
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("router should respond");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
        };
        (status, json)
    }
}

/// A `MessageRecord` with sensible defaults; override fields at the call site.
pub fn message(id: &str, subject: &str, mailbox: &str) -> MessageRecord {
    MessageRecord {
        id: MessageId::from(id),
        source_thread_id: ThreadId::from(format!("thread-{id}")),
        subject: Some(subject.to_string()),
        from_name: Some("Alice".to_string()),
        from_email: Some("alice@example.com".to_string()),
        preview: Some("Preview".to_string()),
        received_at: "2026-03-31T10:00:00Z".to_string(),
        size: 42,
        mailbox_ids: vec![MailboxId::from(mailbox)],
        keywords: vec!["$seen".to_string()],
        body_html: Some("<p>Hello</p>".to_string()),
        body_text: Some("Hello".to_string()),
        rfc_message_id: Some(format!("<{id}@example.test>")),
        ..Default::default()
    }
}
