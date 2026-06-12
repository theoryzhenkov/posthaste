use std::sync::Arc;

use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, RFC3339_EPOCH,
};
use posthaste_store::DatabaseStore;

use crate::util::temp_root;

pub(super) struct Harness {
    pub(super) service: posthaste_domain::MailService,
    pub(super) store: Arc<DatabaseStore>,
}

impl Harness {
    pub(super) fn new() -> Self {
        let root = temp_root("posthaste-stalwart-provider-parity");
        let config_root = root.join("config");
        let state_root = root.join("state");
        let config_repo =
            TomlConfigRepository::open(&config_root).expect("config repository should open");
        config_repo
            .initialize_defaults()
            .expect("config defaults should initialize");
        let store = Arc::new(
            DatabaseStore::open(state_root.join("mail.sqlite"), &state_root)
                .expect("database store should open"),
        );
        let config = Arc::new(config_repo);
        Self {
            service: posthaste_domain::MailService::new(store.clone(), config),
            store,
        }
    }

    pub(super) fn save_account(
        &self,
        id: &str,
        name: &str,
        driver: AccountDriver,
        transport: AccountTransportSettings,
    ) {
        self.service
            .save_source(&AccountSettings {
                id: AccountId::from(id),
                name: name.to_string(),
                full_name: Some("Dev Account".to_string()),
                email_patterns: vec!["dev@example.org".to_string()],
                driver,
                enabled: true,
                appearance: None,
                transport,
                created_at: RFC3339_EPOCH.to_string(),
                updated_at: RFC3339_EPOCH.to_string(),
            })
            .expect("account should save");
    }
}
