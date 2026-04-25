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
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, AppSettings,
    AutomationAction, AutomationBackfillJobStatus, AutomationRule, AutomationTrigger,
    ConfigRepository, MailService, MailStore, SecretRef, SecretStore, SecretStoreError,
    SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
    SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue, RFC3339_EPOCH,
};
use posthaste_server::api::{patch_settings, PatchSettingsRequest};
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
    std::env::temp_dir().join(format!("posthaste-settings-patch-test-{now}-{seq}"))
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

struct SettingsHarness {
    state: Arc<AppState>,
    config_root: PathBuf,
}

impl SettingsHarness {
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
        let service = Arc::new(MailService::new(database_store, config));
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
            config_root,
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

    fn app_toml(&self) -> toml::Value {
        let raw = std::fs::read_to_string(self.config_root.join("app.toml"))
            .expect("app.toml should exist");
        toml::from_str(&raw).expect("app.toml should parse")
    }
}

fn expect_settings_ok(
    result: Result<Json<AppSettings>, posthaste_server::api::ApiError>,
) -> Json<AppSettings> {
    match result {
        Ok(settings) => settings,
        Err(error) => panic!(
            "settings patch should succeed, got {}",
            error.into_response().status()
        ),
    }
}

fn source_rule(account_id: &str) -> AutomationRule {
    AutomationRule {
        id: "rule-newsletters".to_string(),
        name: "Newsletters".to_string(),
        enabled: true,
        triggers: vec![AutomationTrigger::MessageArrived],
        condition: SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: SmartMailboxGroupOperator::All,
                negated: false,
                nodes: vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                    field: SmartMailboxField::SourceId,
                    operator: SmartMailboxOperator::Equals,
                    negated: false,
                    value: SmartMailboxValue::String(account_id.to_string()),
                })],
            },
        },
        actions: vec![AutomationAction::ApplyTag {
            tag: "newsletter".to_string(),
        }],
        backfill: true,
    }
}

#[tokio::test]
async fn patch_settings_automation_rules_preserves_default_account_and_writes_app_toml() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");
    harness
        .state
        .service
        .put_app_settings(&AppSettings {
            default_account_id: Some(AccountId::from("primary")),
            automation_rules: Vec::new(),
            automation_drafts: Vec::new(),
        })
        .expect("settings should save");

    let Json(settings) = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                default_account_id: None,
                automation_rules: Some(vec![source_rule("primary")]),
                automation_drafts: None,
            }),
        )
        .await,
    );

    assert_eq!(
        settings.default_account_id,
        Some(AccountId::from("primary"))
    );
    assert_eq!(settings.automation_rules.len(), 1);
    let backfill_job = harness
        .state
        .service
        .automation_backfill_job_for_current_rules(&AccountId::from("primary"))
        .expect("backfill job should load")
        .expect("backfill job should be queued");
    assert_eq!(backfill_job.status, AutomationBackfillJobStatus::Pending);
    let app_toml = harness.app_toml();
    assert_eq!(app_toml["default_source_id"].as_str(), Some("primary"));
    assert_eq!(
        app_toml["automations"][0]["id"].as_str(),
        Some("rule-newsletters")
    );
}

#[tokio::test]
async fn patch_settings_can_clear_default_account_without_replacing_rules() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");
    harness
        .state
        .service
        .put_app_settings(&AppSettings {
            default_account_id: Some(AccountId::from("primary")),
            automation_rules: vec![source_rule("primary")],
            automation_drafts: Vec::new(),
        })
        .expect("settings should save");

    let Json(settings) = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                default_account_id: Some(None),
                automation_rules: None,
                automation_drafts: None,
            }),
        )
        .await,
    );

    assert_eq!(settings.default_account_id, None);
    assert_eq!(settings.automation_rules.len(), 1);
    let app_toml = harness.app_toml();
    assert!(app_toml.get("default_source_id").is_none());
    assert_eq!(
        app_toml["automations"][0]["id"].as_str(),
        Some("rule-newsletters")
    );
}

#[tokio::test]
async fn patch_settings_persists_incomplete_automation_drafts_without_enqueuing_backfill() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");
    harness
        .state
        .service
        .put_app_settings(&AppSettings {
            default_account_id: Some(AccountId::from("primary")),
            automation_rules: Vec::new(),
            automation_drafts: Vec::new(),
        })
        .expect("settings should save");
    let mut draft = source_rule("primary");
    draft.id = "draft-newsletters".to_string();
    draft.name = String::new();
    draft.actions = vec![AutomationAction::ApplyTag { tag: String::new() }];

    let Json(settings) = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                default_account_id: None,
                automation_rules: None,
                automation_drafts: Some(vec![draft]),
            }),
        )
        .await,
    );

    assert_eq!(settings.automation_rules.len(), 0);
    assert_eq!(settings.automation_drafts.len(), 1);
    assert!(
        harness
            .state
            .service
            .automation_backfill_job_for_current_rules(&AccountId::from("primary"))
            .expect("backfill job should load")
            .is_none()
    );
    let app_toml = harness.app_toml();
    assert_eq!(
        app_toml["draft_automations"][0]["id"].as_str(),
        Some("draft-newsletters")
    );
}

#[tokio::test]
async fn patch_settings_rejects_default_account_that_does_not_exist() {
    let harness = SettingsHarness::new();

    let error = patch_settings(
        State(harness.state.clone()),
        Json(PatchSettingsRequest {
            default_account_id: Some(Some("missing".to_string())),
            automation_rules: None,
            automation_drafts: None,
        }),
    )
    .await
    .expect_err("settings patch should reject missing default account");

    assert_eq!(error.into_response().status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        harness
            .state
            .service
            .get_app_settings()
            .expect("settings should load"),
        AppSettings::default()
    );
}

#[tokio::test]
async fn patch_settings_rejects_invalid_automation_rules_without_persisting() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");
    harness
        .state
        .service
        .put_app_settings(&AppSettings {
            default_account_id: Some(AccountId::from("primary")),
            automation_rules: Vec::new(),
            automation_drafts: Vec::new(),
        })
        .expect("settings should save");
    let mut invalid_rule = source_rule("primary");
    invalid_rule.actions = Vec::new();

    let error = patch_settings(
        State(harness.state.clone()),
        Json(PatchSettingsRequest {
            default_account_id: None,
            automation_rules: Some(vec![invalid_rule]),
            automation_drafts: None,
        }),
    )
    .await
    .expect_err("settings patch should reject invalid automations");

    assert_eq!(error.into_response().status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        harness
            .state
            .service
            .get_app_settings()
            .expect("settings should load"),
        AppSettings {
            default_account_id: Some(AccountId::from("primary")),
            automation_rules: Vec::new(),
            automation_drafts: Vec::new(),
        }
    );
    let app_toml = harness.app_toml();
    assert_eq!(
        app_toml["automations"]
            .as_array()
            .expect("automations should serialize as an array")
            .len(),
        0
    );
}
