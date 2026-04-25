use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, ConfigRepository,
    MailService, MailStore, MailboxId, MailboxRecord, MessageId, MessageRecord, SecretRef,
    SecretStore, SecretStoreError, SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup,
    SmartMailboxGroupOperator, SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode,
    SmartMailboxValue, SyncBatch, SyncWriteStore, ThreadId, RFC3339_EPOCH,
};
use posthaste_server::api::{
    preview_automation_rule, AutomationRulePreviewResponse, PreviewAutomationRuleRequest,
};
use posthaste_server::supervisor::AccountSupervisor;
use posthaste_server::AppState;
use posthaste_store::DatabaseStore;
use tokio::sync::broadcast;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root() -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("posthaste-automation-preview-test-{now}-{seq}"))
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

struct PreviewHarness {
    state: Arc<AppState>,
    store: Arc<DatabaseStore>,
}

impl PreviewHarness {
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
            }),
            store: database_store,
        }
    }

    fn save_account(&self, id: &str, name: &str) {
        self.state
            .service
            .save_source(&AccountSettings {
                id: AccountId::from(id),
                name: name.to_string(),
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

fn message(id: &str, from_name: &str, received_at: &str) -> MessageRecord {
    MessageRecord {
        id: MessageId::from(id),
        source_thread_id: ThreadId::from(format!("thread-{id}")),
        remote_blob_id: None,
        subject: Some(format!("Subject {id}")),
        from_name: Some(from_name.to_string()),
        from_email: Some(format!("{id}@example.test")),
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

fn condition(
    field: SmartMailboxField,
    operator: SmartMailboxOperator,
    value: SmartMailboxValue,
) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
        field,
        operator,
        negated: false,
        value,
    })
}

fn source_is(account_id: &str) -> SmartMailboxRuleNode {
    condition(
        SmartMailboxField::SourceId,
        SmartMailboxOperator::Equals,
        SmartMailboxValue::String(account_id.to_string()),
    )
}

fn from_contains(value: &str) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Group(SmartMailboxGroup {
        operator: SmartMailboxGroupOperator::Any,
        negated: false,
        nodes: vec![
            condition(
                SmartMailboxField::FromName,
                SmartMailboxOperator::Contains,
                SmartMailboxValue::String(value.to_string()),
            ),
            condition(
                SmartMailboxField::FromEmail,
                SmartMailboxOperator::Contains,
                SmartMailboxValue::String(value.to_string()),
            ),
        ],
    })
}

fn rule(nodes: Vec<SmartMailboxRuleNode>) -> SmartMailboxRule {
    SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes,
        },
    }
}

fn expect_preview_ok(
    result: Result<Json<AutomationRulePreviewResponse>, posthaste_server::api::ApiError>,
) -> Json<AutomationRulePreviewResponse> {
    match result {
        Ok(response) => response,
        Err(error) => panic!(
            "automation preview should succeed, got {}",
            error.into_response().status()
        ),
    }
}

#[tokio::test]
async fn preview_automation_rule_returns_total_and_newest_sample() {
    let harness = PreviewHarness::new();
    harness.save_account("primary", "Primary");
    harness.seed_messages(
        "primary",
        vec![
            message("old-match", "Posthaste", "2026-03-31T10:00:00Z"),
            message("new-match", "Posthaste", "2026-04-01T10:00:00Z"),
            message("ignored", "Someone Else", "2026-04-02T10:00:00Z"),
        ],
    );

    let Json(response) = expect_preview_ok(
        preview_automation_rule(
            State(harness.state.clone()),
            Json(PreviewAutomationRuleRequest {
                condition: rule(vec![source_is("primary"), from_contains("Posthaste")]),
                limit: Some(1),
            }),
        )
        .await,
    );

    assert_eq!(response.total, 2);
    assert_eq!(response.items.len(), 1);
    assert_eq!(response.items[0].id, MessageId::from("new-match"));
}

#[tokio::test]
async fn preview_automation_rule_rejects_invalid_limit() {
    let harness = PreviewHarness::new();

    let error = preview_automation_rule(
        State(harness.state.clone()),
        Json(PreviewAutomationRuleRequest {
            condition: rule(Vec::new()),
            limit: Some(0),
        }),
    )
    .await
    .expect_err("preview should reject a zero limit");

    assert_eq!(error.into_response().status(), StatusCode::BAD_REQUEST);
}
