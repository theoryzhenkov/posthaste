use std::sync::Arc;
use std::time::Duration;

use axum::body::to_bytes;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use posthaste_config::TomlConfigRepository;
use posthaste_domain_model::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, MailboxId, MailboxRecord,
    MessageId, MessageRecord, SecretRef, SecretStoreError, SyncBatch, ThreadId, RFC3339_EPOCH,
};
use posthaste_domain_service::{
    ConfigRepository, MailService, MailStore, SecretStore, SyncWriteStore,
};
use posthaste_http_api_adapter::api::{ApiError, ListSourceMessagesQuery};
use posthaste_authority_server::AccountSupervisor;
use posthaste_http_api_adapter::AppState;
use posthaste_store::DatabaseStore;
use posthaste_testkit::temp_root;
use serde_json::Value;
use tokio::sync::broadcast;

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

pub(super) struct ApiHarness {
    // Held only to keep the temp directory alive for the harness's lifetime;
    // removed on drop.
    _root: posthaste_testkit::TempDirGuard,
    pub(super) state: Arc<AppState>,
    service: Arc<MailService>,
    store: Arc<DatabaseStore>,
}

impl ApiHarness {
    pub(super) fn new() -> Self {
        let root = temp_root("posthaste-http-api-adapter-boundary-test");
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
            _root: root,
            state: Arc::new(AppState {
                runtime:
                    posthaste_testkit::runtime_handle_with_account_runtime_provider_for_migration(
                        service.clone(),
                        store.clone(),
                        secret_store.clone(),
                        event_sender,
                        supervisor,
                    ),
                account_logo_root: state_root.join("account-assets/logos"),
                config_root: state_root.to_path_buf(),
                auth_token: "test-token".to_string(),
                macaroon_root_key: posthaste_http_api_adapter::token::RootKey::from_test_bytes([0u8; 32]),
                require_auth: false,
                origin_allowlist: Vec::new(),
                host_allowlist: Vec::new(),
            }),
            service,
            store: database_store,
        }
    }

    pub(super) fn save_account(&self, id: &str) {
        self.service
            .save_source(&AccountSettings {
                id: AccountId::from(id),
                name: id.to_string(),
                full_name: None,
                signature: None,
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

    pub(super) fn seed_messages(&self, account_id: &str, messages: Vec<MessageRecord>) {
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
                    absence_deleted_imap_message_locations: Vec::new(),
                    absence_deleted_message_ids: Vec::new(),
                    deleted_message_ids: Vec::new(),
                    replace_all_mailboxes: true,
                    replace_all_messages: true,
                    cursors: Vec::new(),
                },
            )
            .expect("messages should seed");
    }
}

pub(super) fn message(id: &str, received_at: &str) -> MessageRecord {
    MessageRecord {
        id: MessageId::from(id),
        source_thread_id: ThreadId::from(format!("thread-{id}")),
        subject: Some(format!("Subject {id}")),
        from_name: Some("Sender".to_string()),
        from_email: Some(format!("{id}@example.test")),
        preview: Some(format!("Preview {id}")),
        received_at: received_at.to_string(),
        size: 42,
        mailbox_ids: vec![MailboxId::from("inbox")],
        body_text: Some(format!("Body {id}")),
        rfc_message_id: Some(format!("<{id}@example.test>")),
        ..Default::default()
    }
}

pub(super) fn default_source_messages_query() -> ListSourceMessagesQuery {
    ListSourceMessagesQuery {
        mailbox_id: None,
        limit: None,
        cursor: None,
        sort: None,
        sort_dir: None,
        q: None,
    }
}

pub(super) async fn api_error_json(error: ApiError) -> (StatusCode, Value) {
    response_json(error.into_response()).await
}

pub(super) async fn response_json(response: Response) -> (StatusCode, Value) {
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let json = serde_json::from_slice(&body).expect("response body should be JSON");
    (status, json)
}
