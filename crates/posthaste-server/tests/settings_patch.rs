use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    AccountAppearance, AccountDriver, AccountId, AccountSettings, AccountTransportSettings,
    AppAppearanceSettings, AppPalettePreset, AppSettings, AppThemeMode, AppUiDensity,
    AutomationAction, AutomationBackfillJobStatus, AutomationRule, AutomationTrigger, CachePolicy,
    ConfigRepository, DomainEvent, MailService, MailStore, SecretRef, SecretStore,
    SecretStoreError, SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup,
    SmartMailboxGroupOperator, SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode,
    SmartMailboxValue, EVENT_TOPIC_ACCOUNT_UPDATED, EVENT_TOPIC_CONFIG_RELOADED,
    EVENT_TOPIC_SETTINGS_UPDATED, EVENT_TOPIC_SMART_MAILBOX_CREATED,
    EVENT_TOPIC_SMART_MAILBOX_DELETED, EVENT_TOPIC_SMART_MAILBOX_RESET,
    EVENT_TOPIC_SMART_MAILBOX_UPDATED, RFC3339_EPOCH,
};
use posthaste_server::api::{
    create_smart_mailbox, delete_smart_mailbox, patch_account, patch_settings, patch_smart_mailbox,
    reload_config, reset_default_smart_mailboxes, CreateSmartMailboxRequest, PatchAccountRequest,
    PatchSettingsRequest, PatchSmartMailboxRequest,
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
                oauth_flows: Arc::new(posthaste_server::oauth::OAuthFlowStore::default()),
                auth_token: "test-token".to_string(),
                require_auth: false,
                origin_allowlist: Vec::new(),
                host_allowlist: Vec::new(),
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

fn expect_api_ok<T>(
    result: Result<Json<T>, posthaste_server::api::ApiError>,
    context: &str,
) -> Json<T> {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}, got {}", error.into_response().status()),
    }
}

fn expect_settings_ok(
    result: Result<Json<AppSettings>, posthaste_server::api::ApiError>,
) -> Json<AppSettings> {
    expect_api_ok(result, "settings patch should succeed")
}

fn smart_rule_for_source(account_id: &str) -> SmartMailboxRule {
    SmartMailboxRule {
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
    }
}

fn source_rule(account_id: &str) -> AutomationRule {
    AutomationRule {
        id: "rule-newsletters".to_string(),
        name: "Newsletters".to_string(),
        enabled: true,
        triggers: vec![AutomationTrigger::MessageArrived],
        condition: smart_rule_for_source(account_id),
        actions: vec![AutomationAction::ApplyTag {
            tag: "newsletter".to_string(),
        }],
        backfill: true,
    }
}

async fn receive_event(mut receiver: broadcast::Receiver<DomainEvent>) -> DomainEvent {
    tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("event should arrive")
        .expect("event should be broadcast")
}

#[tokio::test]
async fn patch_settings_publishes_settings_updated_resource_event() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");
    let receiver = harness.state.event_sender.subscribe();

    let _ = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                default_account_id: Some(Some("primary".to_string())),
                automation_rules: None,
                automation_drafts: None,
                cache_policy: None,
                appearance: None,
            }),
        )
        .await,
    );

    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_SETTINGS_UPDATED);
    assert_eq!(event.payload["scope"], "app");
    assert_eq!(
        event.payload["changed"],
        serde_json::json!(["defaultAccount"])
    );
    assert_eq!(event.payload["resources"][0]["kind"], "appSettings");
}

#[tokio::test]
async fn patch_account_appearance_publishes_account_updated_resource_event() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");
    let receiver = harness.state.event_sender.subscribe();

    let _ = expect_api_ok(
        patch_account(
            State(harness.state.clone()),
            Path("primary".to_string()),
            Json(PatchAccountRequest {
                name: None,
                full_name: None,
                email_patterns: None,
                driver: None,
                enabled: None,
                appearance: Some(AccountAppearance::Initials {
                    initials: "Z".to_string(),
                    color_hue: 240,
                }),
                secret: None,
                transport: None,
            }),
        )
        .await,
        "account patch should succeed",
    );

    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_ACCOUNT_UPDATED);
    assert_eq!(event.account_id.as_str(), "primary");
    assert_eq!(event.payload["resources"][0]["kind"], "account");
    assert_eq!(event.payload["resources"][0]["operation"], "updated");
    assert_eq!(event.payload["resources"][0]["id"], "primary");
}

#[tokio::test]
async fn smart_mailbox_mutations_publish_resource_events() {
    let harness = SettingsHarness::new();
    harness.save_account("primary", "Primary");

    let receiver = harness.state.event_sender.subscribe();
    let created = expect_api_ok(
        create_smart_mailbox(
            State(harness.state.clone()),
            Json(CreateSmartMailboxRequest {
                name: "Work".to_string(),
                position: Some(10),
                rule: smart_rule_for_source("primary"),
            }),
        )
        .await,
        "smart mailbox create should succeed",
    )
    .0;
    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_SMART_MAILBOX_CREATED);
    assert_eq!(event.payload["smartMailboxId"], created.id.as_str());
    assert_eq!(event.payload["resources"][0]["operation"], "created");

    let receiver = harness.state.event_sender.subscribe();
    let _ = expect_api_ok(
        patch_smart_mailbox(
            State(harness.state.clone()),
            Path(created.id.as_str().to_string()),
            Json(PatchSmartMailboxRequest {
                name: Some("Work updated".to_string()),
                position: None,
                rule: None,
            }),
        )
        .await,
        "smart mailbox patch should succeed",
    );
    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_SMART_MAILBOX_UPDATED);
    assert_eq!(event.payload["resources"][0]["operation"], "updated");

    let receiver = harness.state.event_sender.subscribe();
    let _ = expect_api_ok(
        delete_smart_mailbox(
            State(harness.state.clone()),
            Path(created.id.as_str().to_string()),
        )
        .await,
        "smart mailbox delete should succeed",
    );
    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_SMART_MAILBOX_DELETED);
    assert_eq!(event.payload["resources"][0]["operation"], "deleted");
}

#[tokio::test]
async fn reload_config_publishes_declarative_resource_event() {
    let harness = SettingsHarness::new();
    std::fs::write(
        harness.config_root.join("sources/reloaded.toml"),
        r#"
id = "reloaded"
name = "Reloaded"
driver = "mock"
enabled = true
"#,
    )
    .expect("source config should write");
    let receiver = harness.state.event_sender.subscribe();

    let _ = expect_api_ok(
        reload_config(State(harness.state.clone())).await,
        "config reload should succeed",
    );

    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_CONFIG_RELOADED);
    assert_eq!(event.account_id.as_str(), "app");
    assert_eq!(event.payload["addedSourceCount"], 1);
    assert_eq!(event.payload["resources"][0]["kind"], "config");
    assert_eq!(event.payload["resources"][0]["operation"], "reloaded");
    assert_eq!(event.payload["resources"][1]["kind"], "account");
    assert_eq!(event.payload["resources"][1]["operation"], "created");
    assert_eq!(event.payload["resources"][1]["id"], "reloaded");
}

#[tokio::test]
async fn reset_default_smart_mailboxes_publishes_resource_event() {
    let harness = SettingsHarness::new();
    let receiver = harness.state.event_sender.subscribe();

    let _ = expect_api_ok(
        reset_default_smart_mailboxes(State(harness.state.clone())).await,
        "reset defaults should succeed",
    );

    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_SMART_MAILBOX_RESET);
    assert_eq!(event.payload["resources"][0]["kind"], "smartMailbox");
    assert_eq!(event.payload["resources"][0]["operation"], "reset");
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
            ..Default::default()
        })
        .expect("settings should save");

    let Json(settings) = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                default_account_id: None,
                cache_policy: None,
                appearance: None,
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
            ..Default::default()
        })
        .expect("settings should save");

    let Json(settings) = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                default_account_id: Some(None),
                cache_policy: None,
                appearance: None,
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
async fn patch_settings_can_update_global_appearance() {
    let harness = SettingsHarness::new();
    let receiver = harness.state.event_sender.subscribe();

    let Json(settings) = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                default_account_id: None,
                cache_policy: None,
                appearance: Some(AppAppearanceSettings {
                    mode: AppThemeMode::Light,
                    palette_preset: AppPalettePreset::Glass,
                    density: AppUiDensity::Comfortable,
                    accent_hue: 210,
                    glass_theme: Default::default(),
                }),
                automation_rules: None,
                automation_drafts: None,
            }),
        )
        .await,
    );

    assert_eq!(settings.appearance.mode, AppThemeMode::Light);
    assert_eq!(settings.appearance.palette_preset, AppPalettePreset::Glass);
    assert_eq!(settings.appearance.density, AppUiDensity::Comfortable);
    assert_eq!(settings.appearance.accent_hue, 210);
    let event = receive_event(receiver).await;
    assert_eq!(event.topic, EVENT_TOPIC_SETTINGS_UPDATED);
    assert_eq!(event.payload["changed"], serde_json::json!(["appearance"]));
    assert_eq!(event.payload["resources"][0]["kind"], "appSettings");
    let app_toml = harness.app_toml();
    assert_eq!(
        app_toml["appearance"]["palettePreset"].as_str(),
        Some("glass")
    );
}

#[tokio::test]
async fn patch_settings_can_update_cache_policy() {
    let harness = SettingsHarness::new();

    let Json(settings) = expect_settings_ok(
        patch_settings(
            State(harness.state.clone()),
            Json(PatchSettingsRequest {
                default_account_id: None,
                cache_policy: Some(CachePolicy {
                    soft_cap_bytes: 64 * 1024 * 1024,
                    hard_cap_bytes: 32 * 1024 * 1024,
                    cache_bodies: true,
                    cache_raw_messages: false,
                    cache_attachments: false,
                }),
                appearance: None,
                automation_rules: None,
                automation_drafts: None,
            }),
        )
        .await,
    );

    assert_eq!(settings.cache_policy.soft_cap_bytes, 64 * 1024 * 1024);
    assert_eq!(settings.cache_policy.hard_cap_bytes, 64 * 1024 * 1024);
    let app_toml = harness.app_toml();
    assert_eq!(
        app_toml["cache"]["soft_cap_bytes"].as_integer(),
        Some(64 * 1024 * 1024)
    );
    assert_eq!(
        app_toml["cache"]["hard_cap_bytes"].as_integer(),
        Some(64 * 1024 * 1024)
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
            ..Default::default()
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
                cache_policy: None,
                appearance: None,
                automation_rules: None,
                automation_drafts: Some(vec![draft]),
            }),
        )
        .await,
    );

    assert_eq!(settings.automation_rules.len(), 0);
    assert_eq!(settings.automation_drafts.len(), 1);
    assert!(harness
        .state
        .service
        .automation_backfill_job_for_current_rules(&AccountId::from("primary"))
        .expect("backfill job should load")
        .is_none());
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
            cache_policy: None,
            appearance: None,
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
            ..Default::default()
        })
        .expect("settings should save");
    let mut invalid_rule = source_rule("primary");
    invalid_rule.actions = Vec::new();

    let error = patch_settings(
        State(harness.state.clone()),
        Json(PatchSettingsRequest {
            default_account_id: None,
            cache_policy: None,
            appearance: None,
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
            ..Default::default()
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
