use std::path::PathBuf;
use std::sync::Arc;

use posthaste_authority_runtime::{build_authority_runtime, RuntimeBuildConfig};
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, RFC3339_EPOCH,
};
use posthaste_store::DatabaseStore;

use crate::paths::temp_root;

/// Disposable integration harness: a config repository, a SQLite store, and a
/// `MailService` bound to them, all rooted under a fresh temp directory.
///
/// The store and service are exposed so tests can drive mutations, flush, sync,
/// and read projections directly. The temp root is cleaned up by the OS; tests
/// should not rely on it surviving the process.
///
/// Future extension (planned, see `docs/testing/L1.md`): a `.with_runtime()`
/// builder that also stands up a `RuntimeCore` against the same store/config so
/// view-settlement assertions can observe the runtime recompute path. The
/// `store`/`service` fields are the seam for that — they already give slice 2
/// everything it needs without changing this constructor.
pub struct Harness {
    pub service: posthaste_domain::MailService,
    pub store: Arc<DatabaseStore>,
    root: PathBuf,
}

impl Harness {
    /// Opens a fresh disposable config + store + service.
    pub fn new() -> Self {
        let root = temp_root("posthaste-testkit-harness");
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
            root,
        }
    }

    /// Stand up an in-process authority runtime against this harness's config
    /// root, with its own state/cache roots under the harness temp dir.
    ///
    /// The harness's `MailService`/`store` are dropped (their connection closed
    /// first) so the runtime opens a fresh store; the runtime owns the store
    /// exposed by [`RuntimeHarness::store`]. Async — the caller owns the tokio
    /// runtime (`#[tokio::test]`).
    pub async fn with_runtime(self) -> crate::runtime::RuntimeHarness {
        let config = RuntimeBuildConfig::new(
            self.root.join("config"),
            self.root.join("runtime-state"),
            self.root.join("runtime-cache"),
        )
        .with_secret_store(Arc::new(crate::runtime::TestSecretStore::default()));
        drop(self);
        let build = build_authority_runtime(config)
            .await
            .expect("authority runtime should build");
        crate::runtime::RuntimeHarness::new(build)
    }

    /// Saves a source account with the given driver and transport settings.
    pub fn save_account(
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

impl Default for Harness {
    fn default() -> Self {
        Self::new()
    }
}
