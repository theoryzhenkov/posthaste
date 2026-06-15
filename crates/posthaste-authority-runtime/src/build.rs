use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    ConfigError, ConfigRepository, DomainEvent, MailService, MailStore, SecretStore, ServiceError,
    StoreError,
};
use posthaste_runtime_contract::{
    RuntimeCaller, RuntimeCore, RuntimeError, RuntimeLifecycle, RuntimeStatus, RuntimeStoreStatus,
};
use posthaste_store::DatabaseStore;
use thiserror::Error;
use tokio::sync::broadcast;

use crate::bootstrap::initialize_config;
use crate::SystemSecretStore;

const DEFAULT_EVENT_CHANNEL_CAPACITY: usize = 512;

/// Transport-free build inputs for the local authority runtime.
///
/// Roots are resolved by the host before construction so the runtime owns mail
/// authority state without depending on renderer storage.
///
/// spec: docs/runtime/L2#runtime-builder-transport-free
/// spec: docs/runtime/L2#runtime-owned-roots
pub struct AuthorityRuntimeBuildConfig {
    pub config_root: PathBuf,
    pub state_root: PathBuf,
    pub cache_root: PathBuf,
    pub bootstrap_path: Option<PathBuf>,
    pub secret_store: Option<Arc<dyn SecretStore>>,
    pub event_channel_capacity: usize,
}

impl AuthorityRuntimeBuildConfig {
    pub fn new(
        config_root: impl Into<PathBuf>,
        state_root: impl Into<PathBuf>,
        cache_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            config_root: config_root.into(),
            state_root: state_root.into(),
            cache_root: cache_root.into(),
            bootstrap_path: None,
            secret_store: None,
            event_channel_capacity: DEFAULT_EVENT_CHANNEL_CAPACITY,
        }
    }

    pub fn with_bootstrap_path(mut self, bootstrap_path: impl Into<PathBuf>) -> Self {
        self.bootstrap_path = Some(bootstrap_path.into());
        self
    }

    pub fn with_bootstrap_path_option(mut self, bootstrap_path: Option<PathBuf>) -> Self {
        self.bootstrap_path = bootstrap_path;
        self
    }

    pub fn with_secret_store(mut self, secret_store: Arc<dyn SecretStore>) -> Self {
        self.secret_store = Some(secret_store);
        self
    }

    pub fn with_event_channel_capacity(mut self, event_channel_capacity: usize) -> Self {
        self.event_channel_capacity = event_channel_capacity;
        self
    }
}

/// Result of building the authority runtime.
///
/// spec: docs/runtime/L2#runtime-handle-transport-neutral
pub struct AuthorityRuntimeBuild {
    pub handle: AuthorityRuntimeHandle,
    pub shutdown: RuntimeShutdownHandle,
    pub runtime_status: RuntimeStatus,
    /// MIGRATION(api-runtime-wrapper): exposes the existing mail service/store
    /// graph to the Axum adapter while handlers move behind runtime methods.
    ///
    /// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#legacy-fields-temporary
    pub api_bridge: AuthorityRuntimeApiMigrationBridge,
}

/// Temporary bridge for the existing Axum API adapter while route handlers move
/// from direct service/store access to runtime-handle methods.
///
/// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#legacy-fields-temporary
#[derive(Clone)]
pub struct AuthorityRuntimeApiMigrationBridge {
    pub service: Arc<MailService>,
    pub store: Arc<dyn MailStore>,
    pub secret_store: Arc<dyn SecretStore>,
    pub event_sender: broadcast::Sender<DomainEvent>,
}

impl AuthorityRuntimeApiMigrationBridge {
    pub fn new(
        service: Arc<MailService>,
        store: Arc<dyn MailStore>,
        secret_store: Arc<dyn SecretStore>,
        event_sender: broadcast::Sender<DomainEvent>,
    ) -> Self {
        Self {
            service,
            store,
            secret_store,
            event_sender,
        }
    }
}

/// Build the authority runtime without binding HTTP or depending on Tauri.
///
/// spec: docs/eph/PLAN-L2-bundled-app-test-plan#authority-runtime-handle-test-first
/// spec: docs/runtime/L2#runtime-builder-transport-free
pub async fn build_authority_runtime(
    config: AuthorityRuntimeBuildConfig,
) -> Result<AuthorityRuntimeBuild, AuthorityRuntimeBuildError> {
    if config.event_channel_capacity == 0 {
        return Err(AuthorityRuntimeBuildError::InvalidConfig(
            "event_channel_capacity must be greater than zero".to_string(),
        ));
    }

    fs::create_dir_all(&config.state_root).map_err(|source| AuthorityRuntimeBuildError::Io {
        path: config.state_root.clone(),
        source,
    })?;
    fs::create_dir_all(&config.cache_root).map_err(|source| AuthorityRuntimeBuildError::Io {
        path: config.cache_root.clone(),
        source,
    })?;

    let config_repo = TomlConfigRepository::open(&config.config_root)?;
    initialize_config(&config_repo, config.bootstrap_path.as_deref())?;

    let database_store = Arc::new(DatabaseStore::open(
        config.state_root.join("mail.sqlite"),
        &config.state_root,
    )?);
    let store: Arc<dyn MailStore> = database_store.clone();
    let config_repo: Arc<dyn ConfigRepository> = Arc::new(config_repo);
    let service = Arc::new(MailService::new(database_store, config_repo.clone()));

    service.sync_source_projections()?;
    let account_count = service.list_sources()?.len();

    let (event_sender, _) = broadcast::channel(config.event_channel_capacity);
    let secret_store = config
        .secret_store
        .unwrap_or_else(|| Arc::new(SystemSecretStore));
    let stopped = Arc::new(AtomicBool::new(false));

    let runtime_status = RuntimeStatus {
        lifecycle: RuntimeLifecycle::Ready,
        store: RuntimeStoreStatus {
            config_loaded: true,
            state_store_open: true,
            cache_root_ready: true,
        },
        account_count,
    };

    let api_bridge =
        AuthorityRuntimeApiMigrationBridge::new(service, store, secret_store, event_sender);
    let core = Arc::new(AuthorityRuntimeCore {
        api_bridge: api_bridge.clone(),
        startup_status: runtime_status.clone(),
        stopped: stopped.clone(),
    });

    Ok(AuthorityRuntimeBuild {
        handle: AuthorityRuntimeHandle { core },
        shutdown: RuntimeShutdownHandle { stopped },
        runtime_status,
        api_bridge,
    })
}

struct AuthorityRuntimeCore {
    #[allow(dead_code)]
    api_bridge: AuthorityRuntimeApiMigrationBridge,
    startup_status: RuntimeStatus,
    stopped: Arc<AtomicBool>,
}

/// Cloneable authority runtime handle used by transport adapters.
///
/// spec: docs/runtime/L2#runtime-handle-transport-neutral
/// spec: docs/backend/L2#handle-methods-transport-free
#[derive(Clone)]
pub struct AuthorityRuntimeHandle {
    core: Arc<AuthorityRuntimeCore>,
}

impl AuthorityRuntimeHandle {
    /// MIGRATION(api-runtime-wrapper): create a runtime handle around existing
    /// test/API parts until all router state is produced by the authority
    /// runtime builder.
    ///
    /// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#appstate-has-runtime-handle
    pub fn from_api_bridge_for_migration(
        api_bridge: AuthorityRuntimeApiMigrationBridge,
        account_count: usize,
    ) -> Self {
        let runtime_status = RuntimeStatus {
            lifecycle: RuntimeLifecycle::Ready,
            store: RuntimeStoreStatus {
                config_loaded: true,
                state_store_open: true,
                cache_root_ready: false,
            },
            account_count,
        };
        Self {
            core: Arc::new(AuthorityRuntimeCore {
                api_bridge,
                startup_status: runtime_status,
                stopped: Arc::new(AtomicBool::new(false)),
            }),
        }
    }

    fn current_status(&self) -> RuntimeStatus {
        let mut status = self.core.startup_status.clone();
        if self.core.stopped.load(Ordering::SeqCst) {
            status.lifecycle = RuntimeLifecycle::Stopped;
        }
        status
    }
}

#[async_trait]
impl RuntimeCore for AuthorityRuntimeHandle {
    async fn runtime_status(&self, _caller: RuntimeCaller) -> Result<RuntimeStatus, RuntimeError> {
        Ok(self.current_status())
    }
}

/// Shutdown ownership for authority runtime tasks and resources.
///
/// The first extraction slice owns no long-lived account tasks yet; this handle
/// records shutdown state so adapters already depend on the runtime-owned
/// shutdown seam instead of tearing resources down themselves.
///
/// spec: docs/runtime/L2#runtime-shutdown-handle
pub struct RuntimeShutdownHandle {
    stopped: Arc<AtomicBool>,
}

impl RuntimeShutdownHandle {
    pub async fn shutdown(self) -> Result<(), AuthorityRuntimeShutdownError> {
        self.stopped.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum AuthorityRuntimeBuildError {
    #[error("config error: {0}")]
    Config(#[from] ConfigError),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("service error: {0}")]
    Service(#[from] ServiceError),
    #[error("invalid runtime build config: {0}")]
    InvalidConfig(String),
    #[error("io error for {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("failed to read bootstrap config {path}: {source}")]
    BootstrapRead { path: PathBuf, source: io::Error },
    #[error("failed to parse bootstrap config {path}: {message}")]
    BootstrapParse { path: PathBuf, message: String },
    #[error("failed to read runtime clock: {0}")]
    Clock(String),
}

#[derive(Debug, Error)]
pub enum AuthorityRuntimeShutdownError {
    #[error("runtime shutdown failed: {0}")]
    Failed(String),
}
