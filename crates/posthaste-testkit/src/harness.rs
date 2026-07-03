use std::sync::Arc;
use std::time::Duration;

use posthaste_authority_server::build_authority_server;
use posthaste_config::TomlConfigRepository;
use posthaste_domain_model::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, RFC3339_EPOCH,
};
use posthaste_runtime::RuntimeBuildConfig;
use posthaste_store::DatabaseStore;

use crate::guard::TempDirGuard;
use crate::paths::temp_root;

/// Disposable integration harness: a config repository, a SQLite store, and a
/// `MailService` bound to them, all rooted under a fresh temp directory.
///
/// The store and service are exposed so tests can drive mutations, flush, sync,
/// and read projections directly. The temp root is a [`TempDirGuard`] guard (P6):
/// it is removed on drop, including a panicking unwind, so a failing test
/// leaves nothing behind in `$TMPDIR`.
///
/// `with_runtime()` consumes this harness and stands up an in-process
/// `RuntimeApi` against the same config root (see `docs/testing/L1.md`),
/// exposing the runtime handle, store, and event bus so view-settlement
/// assertions can observe the recompute path. The `store`/`service` fields are
/// the seam for direct driving without the runtime.
pub struct Harness {
    pub service: posthaste_domain_service::MailService,
    pub store: Arc<DatabaseStore>,
    root: TempDirGuard,
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
            service: posthaste_domain_service::MailService::new(store.clone(), config),
            store,
            root,
        }
    }

    /// Stand up an in-process authority runtime against this harness's config
    /// root, with its own state/cache roots under the harness temp dir.
    ///
    /// The harness's `MailService`/`store` are dropped (their connection closed
    /// first) so the runtime opens a fresh store; the runtime owns the store
    /// exposed by [`RuntimeHarness::store`]. The temp-root guard is handed to
    /// the returned [`RuntimeHarness`] rather than dropped here — dropping it
    /// early would delete the config/state directories out from under the
    /// runtime `build_authority_server` is about to open. Async — the caller
    /// owns the tokio runtime (`#[tokio::test]`).
    pub async fn with_runtime(self) -> crate::runtime::RuntimeHarness {
        let Harness {
            service,
            store,
            root,
        } = self;
        let config = RuntimeBuildConfig::new(
            root.join("config"),
            root.join("runtime-state"),
            root.join("runtime-cache"),
        )
        .with_secret_store(Arc::new(crate::runtime::TestSecretStore::default()))
        .with_poll_interval(Duration::from_millis(500));
        drop(service);
        drop(store);
        let build = build_authority_server(config)
            .await
            .expect("authority runtime should build");
        crate::runtime::RuntimeHarness::new(build, root)
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
                signature: None,
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
