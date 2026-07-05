use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::response::IntoResponse;
use axum::Json;
use posthaste_authority_server::AccountSupervisor;
use posthaste_config::TomlConfigRepository;
use posthaste_domain_model::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, AppSettings,
    AutomationAction, AutomationRule, AutomationTrigger, DomainEvent, SecretRef, SecretStoreError,
    SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
    SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue, RFC3339_EPOCH,
};
use posthaste_domain_service::{ConfigRepository, MailService, MailStore, SecretStore};
use posthaste_http_api_adapter::AppState;
use posthaste_store::DatabaseStore;
use posthaste_testkit::temp_root;
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

pub(super) struct SettingsHarness {
    // Held only to keep the temp directory alive for the harness's lifetime;
    // removed on drop.
    _root: posthaste_testkit::TempDirGuard,
    pub(super) state: Arc<AppState>,
    pub(super) config_root: PathBuf,
    pub(super) service: Arc<MailService>,
    event_sender: broadcast::Sender<DomainEvent>,
}

impl SettingsHarness {
    pub(super) fn new() -> Self {
        let root = temp_root("posthaste-settings-patch-test");
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
            _root: root,
            state: Arc::new(AppState {
                runtime:
                    posthaste_testkit::runtime_handle_with_account_runtime_provider_for_migration(
                        service.clone(),
                        store.clone(),
                        secret_store.clone(),
                        event_sender.clone(),
                        supervisor,
                    ),
                account_logo_root: state_root.join("account-assets/logos"),
                config_root: state_root.clone(),
                auth_token: "test-token".to_string(),
                macaroon_root_key: posthaste_http_api_adapter::token::RootKey::from_test_bytes(
                    [0u8; 32],
                ),
                require_auth: false,
                origin_allowlist: Vec::new(),
                host_allowlist: Vec::new(),
            }),
            config_root,
            service,
            event_sender,
        }
    }

    pub(super) fn save_account(&self, id: &str, name: &str) {
        self.service
            .save_source(&AccountSettings {
                id: AccountId::from(id),
                name: name.to_string(),
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

    pub(super) fn subscribe_events(&self) -> broadcast::Receiver<DomainEvent> {
        self.event_sender.subscribe()
    }

    pub(super) fn app_toml(&self) -> toml::Value {
        let raw = std::fs::read_to_string(self.config_root.join("app.toml"))
            .expect("app.toml should exist");
        toml::from_str(&raw).expect("app.toml should parse")
    }
}

pub(super) fn expect_api_ok<T>(
    result: Result<Json<T>, posthaste_http_api_adapter::api::ApiError>,
    context: &str,
) -> Json<T> {
    match result {
        Ok(value) => value,
        Err(error) => panic!("{context}, got {}", error.into_response().status()),
    }
}

pub(super) fn expect_settings_ok(
    result: Result<Json<AppSettings>, posthaste_http_api_adapter::api::ApiError>,
) -> Json<AppSettings> {
    expect_api_ok(result, "settings patch should succeed")
}

pub(super) fn smart_rule_for_source(account_id: &str) -> SmartMailboxRule {
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

pub(super) fn source_rule(account_id: &str) -> AutomationRule {
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

pub(super) async fn receive_event(mut receiver: broadcast::Receiver<DomainEvent>) -> DomainEvent {
    tokio::time::timeout(Duration::from_secs(1), receiver.recv())
        .await
        .expect("event should arrive")
        .expect("event should be broadcast")
}
