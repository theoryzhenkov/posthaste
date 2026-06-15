use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    AccountId, AppSettings, ConfigError, ConfigRepository, DomainEvent, MailService, MailStore,
    MessageId, SecretStore, ServiceError, ServiceErrorKind, SmartMailboxId, StoreError, SyncMode,
};
use posthaste_runtime_contract::{
    AccountScopeRequest, RuntimeAccountList, RuntimeAdapterError, RuntimeCaller, RuntimeCore,
    RuntimeError, RuntimeErrorCode, RuntimeLifecycle, RuntimeStatus, RuntimeStoreStatus,
};
use posthaste_store::DatabaseStore;
use thiserror::Error;
use tokio::sync::broadcast;

use crate::account_reads::{AccountReadService, DefaultAccountRuntimeOverviewProvider};
use crate::bootstrap::initialize_config;
use crate::{
    AccountRuntimeOverviewProvider, LiveAccountRuntimeProvider, SystemSecretStore,
    UnavailableLiveAccountRuntimeProvider,
};

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
    let account_reads = Arc::new(AccountReadService::new(
        api_bridge.service.clone(),
        Arc::new(DefaultAccountRuntimeOverviewProvider),
    ));
    let core = Arc::new(AuthorityRuntimeCore {
        api_bridge: api_bridge.clone(),
        account_reads,
        live_accounts: Arc::new(UnavailableLiveAccountRuntimeProvider),
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
    account_reads: Arc<AccountReadService>,
    live_accounts: Arc<dyn LiveAccountRuntimeProvider>,
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
    fn store_error_to_runtime_error(error: StoreError) -> RuntimeError {
        RuntimeError(RuntimeAdapterError {
            code: RuntimeErrorCode::Internal,
            message: error.to_string(),
            retryable: false,
            correlation_id: None,
            details: serde_json::Value::Null,
        })
    }

    fn service_error_to_runtime_error(error: ServiceError) -> RuntimeError {
        let code = match error.kind() {
            ServiceErrorKind::NotFound => RuntimeErrorCode::NotFound,
            ServiceErrorKind::Conflict | ServiceErrorKind::StateMismatch => {
                RuntimeErrorCode::Conflict
            }
            ServiceErrorKind::AuthError => RuntimeErrorCode::Unauthorized,
            ServiceErrorKind::GatewayUnavailable | ServiceErrorKind::NetworkError => {
                RuntimeErrorCode::ProviderUnavailable
            }
            ServiceErrorKind::CannotCalculateChanges
            | ServiceErrorKind::GatewayRejected
            | ServiceErrorKind::SecretUnavailable
            | ServiceErrorKind::SecretUnsupported
            | ServiceErrorKind::StorageFailure
            | ServiceErrorKind::ConfigValidation
            | ServiceErrorKind::ConfigIo
            | ServiceErrorKind::ConfigParse => RuntimeErrorCode::Internal,
        };
        RuntimeError(RuntimeAdapterError {
            code,
            message: error.to_string(),
            retryable: false,
            correlation_id: None,
            details: serde_json::Value::Null,
        })
    }

    /// MIGRATION(api-runtime-wrapper): create a runtime handle around existing
    /// test/API parts until all router state is produced by the authority
    /// runtime builder.
    ///
    /// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#appstate-has-runtime-handle
    pub fn from_api_bridge_for_migration(
        api_bridge: AuthorityRuntimeApiMigrationBridge,
        account_count: usize,
    ) -> Self {
        Self::from_api_bridge_with_status_provider_for_migration(
            api_bridge,
            account_count,
            Arc::new(DefaultAccountRuntimeOverviewProvider),
        )
    }

    pub fn from_api_bridge_with_status_provider_for_migration(
        api_bridge: AuthorityRuntimeApiMigrationBridge,
        account_count: usize,
        status_provider: Arc<dyn AccountRuntimeOverviewProvider>,
    ) -> Self {
        Self::from_api_bridge_with_providers_for_migration(
            api_bridge,
            account_count,
            status_provider,
            Arc::new(UnavailableLiveAccountRuntimeProvider),
        )
    }

    pub fn from_api_bridge_with_providers_for_migration(
        api_bridge: AuthorityRuntimeApiMigrationBridge,
        account_count: usize,
        status_provider: Arc<dyn AccountRuntimeOverviewProvider>,
        live_accounts: Arc<dyn LiveAccountRuntimeProvider>,
    ) -> Self {
        let account_reads = Arc::new(AccountReadService::new(
            api_bridge.service.clone(),
            status_provider,
        ));
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
                account_reads,
                live_accounts,
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

    async fn get_app_settings(&self, _caller: RuntimeCaller) -> Result<AppSettings, RuntimeError> {
        self.core
            .account_reads
            .app_settings()
            .map_err(Self::service_error_to_runtime_error)
    }

    async fn list_accounts(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<RuntimeAccountList, RuntimeError> {
        self.core
            .account_reads
            .list_accounts()
            .await
            .map_err(Self::service_error_to_runtime_error)
    }

    async fn get_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<posthaste_domain::AccountOverview, RuntimeError> {
        self.core
            .account_reads
            .get_account(account_id)
            .await
            .map_err(Self::service_error_to_runtime_error)?
            .ok_or_else(|| {
                RuntimeError(RuntimeAdapterError {
                    code: RuntimeErrorCode::NotFound,
                    message: "account not found".to_string(),
                    retryable: false,
                    correlation_id: None,
                    details: serde_json::Value::Null,
                })
            })
    }

    async fn resolve_account_scope(
        &self,
        _caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<Vec<AccountId>, RuntimeError> {
        self.core
            .account_reads
            .resolve_account_scope(scope)
            .map_err(Self::service_error_to_runtime_error)
    }

    async fn list_mailboxes(
        &self,
        _caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<
        std::collections::BTreeMap<AccountId, Vec<posthaste_domain::MailboxSummary>>,
        RuntimeError,
    > {
        self.core
            .account_reads
            .list_mailboxes(scope)
            .map_err(Self::service_error_to_runtime_error)
    }

    async fn list_smart_mailboxes(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<posthaste_domain::SmartMailboxSummary>, RuntimeError> {
        self.core
            .account_reads
            .list_smart_mailboxes()
            .map_err(Self::service_error_to_runtime_error)
    }

    async fn get_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<posthaste_domain::SmartMailbox, RuntimeError> {
        self.core
            .account_reads
            .get_smart_mailbox(&smart_mailbox_id)
            .map_err(Self::service_error_to_runtime_error)
    }

    async fn list_tags(
        &self,
        _caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<Vec<posthaste_domain::TagSummary>, RuntimeError> {
        self.core
            .account_reads
            .list_tags(scope)
            .map_err(Self::service_error_to_runtime_error)
    }

    async fn get_identity(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<posthaste_domain::Identity, RuntimeError> {
        let gateway = self
            .core
            .live_accounts
            .gateway(&account_id)
            .await
            .map_err(Self::service_error_to_runtime_error)?;
        self.core
            .api_bridge
            .service
            .fetch_identity(&account_id, gateway.as_ref())
            .await
            .map_err(Self::service_error_to_runtime_error)
    }

    async fn list_sender_addresses(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<posthaste_domain::CachedSenderAddress>, RuntimeError> {
        self.core
            .api_bridge
            .store
            .list_sender_address_cache()
            .map_err(Self::store_error_to_runtime_error)
    }

    async fn get_reply_context(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<posthaste_domain::ReplyContext, RuntimeError> {
        let gateway = self
            .core
            .live_accounts
            .gateway(&account_id)
            .await
            .map_err(Self::service_error_to_runtime_error)?;
        self.core
            .api_bridge
            .service
            .fetch_reply_context(&account_id, &message_id, gateway.as_ref())
            .await
            .map_err(Self::service_error_to_runtime_error)
    }

    async fn sync_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        mode: SyncMode,
    ) -> Result<usize, RuntimeError> {
        self.core
            .live_accounts
            .sync_account_with_mode(&account_id, mode)
            .await
            .map_err(Self::service_error_to_runtime_error)
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
