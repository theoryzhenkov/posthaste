use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::to_bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, ConfigRepository,
    MailService, MailStore, MailboxId, MailboxRecord, MessageId, MessageRecord, SecretRef,
    SecretStore, SecretStoreError, SyncBatch, SyncWriteStore, ThreadId, RFC3339_EPOCH,
};
use posthaste_server::api::{
    health, list_mailboxes, list_source_messages, ApiError, ListSourceMessagesQuery,
};
use posthaste_server::supervisor::AccountSupervisor;
use posthaste_server::AppState;
use posthaste_store::DatabaseStore;
use serde_json::Value;
use tokio::sync::broadcast;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("posthaste-api-boundary-test-{now}-{seq}"))
}

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

struct ApiHarness {
    state: Arc<AppState>,
    store: Arc<DatabaseStore>,
}

impl ApiHarness {
    fn new() -> Self {
        let root = temp_root();
        let config_root = root.join("config");
        let state_root = root.join("state");
        let config_repo =
            TomlConfigRepository::open(&config_root).expect("config repository should open");
        config_repo
            .initialize_defaults()
            .expect("config defaults should initialize");
        let database_store = Arc::new(
            DatabaseStore::open(state_root.join("mail.sqlite"), &state_root)
                .expect("database store should open"),
        );
        let store: Arc<dyn MailStore> = database_store.clone();
        let config: Arc<dyn ConfigRepository> = Arc::new(config_repo);
        let service = Arc::new(MailService::new(database_store.clone(), config));
        let (event_sender, _) = broadcast::channel(16);
        let secret_store: Arc<dyn SecretStore> = Arc::new(TestSecretStore);
        let supervisor = Arc::new(AccountSupervisor::new(
            service.clone(),
            store.clone(),
            secret_store.clone(),
            event_sender.clone(),
            Duration::from_secs(60),
        ));
        Self {
            state: Arc::new(AppState {
                service,
                store,
                secret_store,
                supervisor,
                event_sender,
                account_logo_root: state_root.join("account-assets/logos"),
                oauth_flows: Arc::new(posthaste_server::oauth::OAuthFlowStore::default()),
            }),
            store: database_store,
        }
    }

    fn save_account(&self, id: &str) {
        self.state
            .service
            .save_source(&AccountSettings {
                id: AccountId::from(id),
                name: id.to_string(),
                full_name: None,
                email_patterns: Vec::new(),
                driver: AccountDriver::Mock,
                enabled: true,
                appearance: None,
                transport: AccountTransportSettings::default(),
                created_at: RFC3339_EPOCH.to_string(),
                updated_at: RFC3339_EPOCH.to_string(),
            })
            .expect("account should save");
    }

    fn seed_messages(&self, account_id: &str, messages: Vec<MessageRecord>) {
        self.store
            .apply_sync_batch(
                &AccountId::from(account_id),
                &SyncBatch {
                    mailboxes: vec![MailboxRecord {
                        id: MailboxId::from("inbox"),
                        name: "Inbox".to_string(),
                        role: Some("inbox".to_string()),
                        unread_emails: messages.len() as i64,
                        total_emails: messages.len() as i64,
                    }],
                    messages,
                    imap_mailbox_states: Vec::new(),
                    imap_message_locations: Vec::new(),
                    deleted_imap_message_locations: Vec::new(),
                    deleted_mailbox_ids: Vec::new(),
                    deleted_message_ids: Vec::new(),
                    replace_all_mailboxes: true,
                    replace_all_messages: true,
                    cursors: Vec::new(),
                },
            )
            .expect("messages should seed");
    }
}

fn message(id: &str, received_at: &str) -> MessageRecord {
    MessageRecord {
        id: MessageId::from(id),
        source_thread_id: ThreadId::from(format!("thread-{id}")),
        remote_blob_id: None,
        subject: Some(format!("Subject {id}")),
        from_name: Some("Sender".to_string()),
        from_email: Some(format!("{id}@example.test")),
        to: Vec::new(),
        preview: Some(format!("Preview {id}")),
        received_at: received_at.to_string(),
        has_attachment: false,
        size: 42,
        mailbox_ids: vec![MailboxId::from("inbox")],
        keywords: Vec::new(),
        body_html: None,
        body_text: Some(format!("Body {id}")),
        raw_mime: None,
        rfc_message_id: Some(format!("<{id}@example.test>")),
        in_reply_to: None,
        references: Vec::new(),
    }
}

fn default_source_messages_query() -> ListSourceMessagesQuery {
    ListSourceMessagesQuery {
        mailbox_id: None,
        limit: None,
        cursor: None,
        sort: None,
        sort_dir: None,
        q: None,
    }
}

async fn api_error_json(error: ApiError) -> (StatusCode, Value) {
    response_json(error.into_response()).await
}

async fn response_json(response: Response) -> (StatusCode, Value) {
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let json = serde_json::from_slice(&body).expect("response body should be JSON");
    (status, json)
}

// spec: docs/L1-api#health
#[tokio::test]
async fn health_returns_only_product_readiness_status() {
    let (status, body) = response_json(health().await.into_response()).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, serde_json::json!({ "status": "ok" }));
}

// spec: docs/L0-testing#api-boundary-contracts
#[tokio::test]
async fn source_message_page_returns_structured_not_found_for_unknown_source() {
    let harness = ApiHarness::new();

    let error = list_source_messages(
        State(harness.state.clone()),
        Path("missing".to_string()),
        HeaderMap::new(),
        Query(default_source_messages_query()),
    )
    .await
    .expect_err("unknown source should fail");

    let (status, body) = api_error_json(error).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
    assert_eq!(body["message"], "account not found");
    assert_eq!(body["details"], serde_json::json!({}));
}

// spec: docs/L0-testing#api-boundary-contracts
#[tokio::test]
async fn source_mailboxes_return_structured_not_found_for_unknown_source() {
    let harness = ApiHarness::new();

    let error = list_mailboxes(State(harness.state.clone()), Path("missing".to_string()))
        .await
        .expect_err("unknown source should fail");

    let (status, body) = api_error_json(error).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
    assert_eq!(body["message"], "account not found");
    assert_eq!(body["details"], serde_json::json!({}));
}

// spec: docs/L0-testing#api-boundary-contracts
#[tokio::test]
async fn source_message_page_rejects_cursor_issued_for_another_source() {
    let harness = ApiHarness::new();
    harness.save_account("primary");
    harness.save_account("secondary");
    harness.seed_messages(
        "primary",
        vec![
            message("primary-new", "2026-04-02T10:00:00Z"),
            message("primary-old", "2026-04-01T10:00:00Z"),
        ],
    );
    harness.seed_messages(
        "secondary",
        vec![
            message("secondary-new", "2026-04-02T11:00:00Z"),
            message("secondary-old", "2026-04-01T11:00:00Z"),
        ],
    );
    let mut first_page_query = default_source_messages_query();
    first_page_query.limit = Some(1);
    let Json(first_page) = match list_source_messages(
        State(harness.state.clone()),
        Path("secondary".to_string()),
        HeaderMap::new(),
        Query(first_page_query),
    )
    .await
    {
        Ok(page) => page,
        Err(error) => panic!(
            "secondary page should load, got {}",
            error.into_response().status()
        ),
    };
    let cursor = first_page
        .next_cursor
        .expect("first secondary page should include a cursor");

    let mut cross_source_query = default_source_messages_query();
    cross_source_query.cursor = Some(cursor);
    let error = list_source_messages(
        State(harness.state.clone()),
        Path("primary".to_string()),
        HeaderMap::new(),
        Query(cross_source_query),
    )
    .await
    .expect_err("cursor from another source should fail");

    let (status, body) = api_error_json(error).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "invalid_cursor");
    assert_eq!(body["details"], serde_json::json!({}));
}
