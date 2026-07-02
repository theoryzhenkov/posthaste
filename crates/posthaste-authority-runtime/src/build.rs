//! Far-node assembly: builds the backend (store + provider engine + IMAP) and
//! composes a runtime over it via [`posthaste_runtime::assemble_runtime`]. The
//! near node itself (handle, views, sessions, read cache, outbox, the remote
//! transport, and `build_remote_runtime`) lives in `posthaste-runtime`.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use posthaste_config::TomlConfigRepository;
use posthaste_domain_service::{ConfigRepository, DomainEvent, MailService, MailStore, SecretStore};
use posthaste_link_contract::{BackendApi, BackendLink};
use posthaste_contract_core::{RuntimeLifecycle, RuntimeStatus, RuntimeStoreStatus};
use posthaste_store::DatabaseStore;
use tokio::sync::broadcast;

use posthaste_runtime::{
    assemble_runtime, BackendTransportConfig, BackendTransportDecorator, ReadCache, RemoteBackend,
    RuntimeAssembly, RuntimeBuildConfig, RuntimeBuildError, RuntimeHandle, RuntimeShutdownHandle,
    SystemSecretStore,
};

use crate::account_reads::{AccountReadService, DefaultAccountRuntimeOverviewProvider};
use crate::account_repository::AccountRepository;
use crate::backend::Backend;
use crate::bootstrap::initialize_config;
use crate::local_backend::LocalBackend;
use crate::mail_queries::MailQueryService;
use crate::mutations::AccountMutationService;
use crate::{
    AccountRuntimeOverviewProvider, AccountSupervisor, LiveAccountRuntimeProvider,
    UnavailableLiveAccountRuntimeProvider,
};

/// Result of building the authority runtime in-process (backend + near node).
pub struct AuthorityRuntimeBuild {
    pub handle: RuntimeHandle,
    pub shutdown: RuntimeShutdownHandle,
    pub runtime_status: RuntimeStatus,
    pub account_supervisor: Arc<AccountSupervisor>,
    /// The backend's account-mutation service, surfaced so the host can run the
    /// OAuth holdout (account creation from a provider exchange) directly — it
    /// is a backend operation, not part of the renderer-facing handle.
    pub account_mutations: Arc<AccountMutationService>,
    /// Runtime-owned secret store exposed for host-owned auth-token setup.
    pub secret_store: Arc<dyn SecretStore>,
    /// MIGRATION(api-runtime-wrapper): exposes the existing mail service/store
    /// graph for compatibility harnesses and migration handle constructors.
    ///
    /// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#legacy-fields-temporary
    pub api_bridge: AuthorityRuntimeApiMigrationBridge,
    /// The in-process runtime↔backend link. A split-backend host serves its
    /// transport over the link wire (`link_router`) so a remote runtime can
    /// connect; the in-process runtime already holds the same link internally.
    ///
    /// @spec docs/replication/backend-link/L1#3-the-backendapi-contract
    pub backend_link: BackendLink,
}

/// Temporary bridge for compatibility harnesses and migration handle
/// constructors while direct service/store access is retired from API state.
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
/// The bundled/daemon composition ([replication backend-link L2 §7](../replication/backend-link/L2.md)):
/// build the backend far node, then the runtime near node over it (in-process
/// link by default). Behavior-preserving — the same graph as before, factored so
/// the two roles can also be composed independently by the lean binaries.
///
/// spec: docs/eph/PLAN-L2-bundled-app-test-plan#authority-runtime-handle-test-first
/// spec: docs/runtime/internals/L2#runtime-builder-transport-free
pub async fn build_authority_runtime(
    config: RuntimeBuildConfig,
) -> Result<AuthorityRuntimeBuild, RuntimeBuildError> {
    let backend = build_backend(&config).await?;
    Ok(build_runtime(backend, config))
}

/// A standalone backend far node ([replication backend-link L2 §7](../replication/backend-link/L2.md),
/// assertion `backend-builds-standalone`): the store + service + account
/// supervisor + the L4 [`Backend`], with NO runtime near node. The
/// `posthaste-backend` role binary serves [`transport`](BackendNode::transport)
/// over `link_router` so a remote runtime drives it across the link.
pub struct BackendNode {
    transport: Arc<dyn BackendApi>,
    /// Held so the supervisor's account tasks keep running for the node's life
    /// (also reachable through the transport's far node).
    _account_supervisor: Arc<AccountSupervisor>,
    runtime_status: RuntimeStatus,
}

impl BackendNode {
    /// The in-process link transport over this backend — hand to `link_router`.
    pub fn transport(&self) -> Arc<dyn BackendApi> {
        self.transport.clone()
    }

    /// The backend's startup status (store readiness + account count).
    pub fn runtime_status(&self) -> &RuntimeStatus {
        &self.runtime_status
    }
}

/// Build a standalone backend far node (no runtime). The far node is live after
/// this returns (the supervisor has started its accounts); serve
/// [`BackendNode::transport`] over the link to expose it to a remote runtime.
pub async fn build_backend_node(
    config: RuntimeBuildConfig,
) -> Result<BackendNode, RuntimeBuildError> {
    let backend = build_backend(&config).await?;
    let transport: Arc<dyn BackendApi> = Arc::new(LocalBackend::new(backend.backend.clone()));
    Ok(BackendNode {
        transport,
        _account_supervisor: backend.account_supervisor.clone(),
        runtime_status: backend.runtime_status,
    })
}

/// A lean runtime near node ([replication backend-link L2 §7](../replication/backend-link/L2.md)): the
/// session / view / outbox machinery over a REMOTE backend link, with NO local
/// backend (no store, service, or supervisor). The `posthaste-runtime` role (the
/// daemon configured with a remote backend) builds this — reads + writes cross
/// the link, and the down-channel bridge keeps the cache and views live.
pub(crate) struct BackendBuild {
    secret_store: Arc<dyn SecretStore>,
    event_sender: broadcast::Sender<DomainEvent>,
    account_supervisor: Arc<AccountSupervisor>,
    account_mutations: Arc<AccountMutationService>,
    backend: Arc<Backend>,
    api_bridge: AuthorityRuntimeApiMigrationBridge,
    runtime_status: RuntimeStatus,
}

/// Build the backend far node alone (no runtime). Used directly by a
/// backend-only deployment and as the first half of the bundled build.
pub(crate) async fn build_backend(
    config: &RuntimeBuildConfig,
) -> Result<BackendBuild, RuntimeBuildError> {
    if config.event_channel_capacity == 0 {
        return Err(RuntimeBuildError::InvalidConfig(
            "event_channel_capacity must be greater than zero".to_string(),
        ));
    }

    fs::create_dir_all(&config.state_root).map_err(|source| RuntimeBuildError::Io {
        path: config.state_root.clone(),
        source,
    })?;
    fs::create_dir_all(&config.cache_root).map_err(|source| RuntimeBuildError::Io {
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
        .clone()
        .unwrap_or_else(|| Arc::new(SystemSecretStore));

    let runtime_status = RuntimeStatus {
        lifecycle: RuntimeLifecycle::Ready,
        store: RuntimeStoreStatus {
            config_loaded: true,
            state_store_open: true,
            cache_root_ready: true,
        },
        account_count,
    };

    let api_bridge = AuthorityRuntimeApiMigrationBridge::new(
        service.clone(),
        store.clone(),
        secret_store.clone(),
        event_sender.clone(),
    );
    let account_supervisor = Arc::new(AccountSupervisor::new(
        service.clone(),
        store.clone(),
        secret_store.clone(),
        event_sender.clone(),
        config.poll_interval,
    ));
    for source in service.list_sources()? {
        account_supervisor.start_account(&source).await;
    }
    let account_reads = Arc::new(AccountReadService::new(
        api_bridge.service.clone(),
        account_supervisor.clone(),
    ));
    let account_repository = Arc::new(AccountRepository::new(
        api_bridge.service.clone(),
        api_bridge.secret_store.clone(),
    ));
    let account_mutations = Arc::new(AccountMutationService::new(
        api_bridge.service.clone(),
        api_bridge.store.clone(),
        account_repository,
        api_bridge.event_sender.clone(),
        account_supervisor.clone(),
        account_reads.clone(),
    ));
    let mail_queries = Arc::new(MailQueryService::new(
        api_bridge.service.clone(),
        account_supervisor.clone(),
    ));
    let backend = Arc::new(Backend::new(
        service.clone(),
        store.clone(),
        mail_queries,
        account_reads,
        Some(account_mutations.clone()),
        account_supervisor.clone(),
        event_sender.clone(),
    ));

    Ok(BackendBuild {
        secret_store,
        event_sender,
        account_supervisor,
        account_mutations,
        backend,
        api_bridge,
        runtime_status,
    })
}

/// Compose a runtime near node over the in-process backend
/// ([replication backend-link L2 §7](../replication/backend-link/L2.md)). The link
/// is config-selected (in-process over the backend, or remote); the read cache
/// follows the same selection. Surfaces the backend's account-mutation service
/// for the OAuth holdout. Must run within a Tokio runtime.
pub(crate) fn build_runtime(
    backend: BackendBuild,
    config: RuntimeBuildConfig,
) -> AuthorityRuntimeBuild {
    let BackendBuild {
        secret_store,
        event_sender,
        account_supervisor,
        account_mutations,
        backend,
        api_bridge,
        runtime_status,
    } = backend;
    // Take the transport selection out of the config (the override decorator is
    // `FnOnce`, so it is moved, not cloned); the rest of the config was consumed
    // by `build_backend`.
    let RuntimeBuildConfig {
        backend_transport,
        backend_transport_override,
        ..
    } = config;

    let backend_link = select_backend_link(
        &backend_transport,
        backend_transport_override,
        backend.clone(),
    );
    let reads = Arc::new(build_read_cache(
        &backend_transport,
        &backend,
        &backend_link,
    ));
    // A split runtime drives its cache + views from the backend down-channel; an
    // in-process runtime shares the backend's bus, so no bridge is needed.
    let drive_down_channel = matches!(backend_transport, BackendTransportConfig::Remote { .. });
    let composed = assemble_runtime(RuntimeAssembly {
        backend_link: backend_link.clone(),
        reads,
        event_sender,
        startup_status: runtime_status.clone(),
        drive_down_channel,
    });

    AuthorityRuntimeBuild {
        handle: composed.handle,
        shutdown: composed.shutdown,
        runtime_status,
        account_supervisor,
        account_mutations,
        secret_store,
        api_bridge,
        backend_link,
    }
}

fn build_read_cache(
    transport: &BackendTransportConfig,
    backend: &Arc<Backend>,
    backend_link: &BackendLink,
) -> ReadCache {
    match transport {
        // Remote: read through the same RemoteBackend the link uses (shared HTTP
        // client), retaining what flows back. In-process: read straight through
        // a LocalBackend over the co-located far node, retaining nothing.
        BackendTransportConfig::Remote { .. } => {
            ReadCache::retaining(backend_link.transport().clone())
        }
        BackendTransportConfig::InProcess => {
            ReadCache::passthrough(Arc::new(LocalBackend::new(backend.clone())))
        }
    }
}

/// Build the runtime↔backend [`BackendLink`] over its config-selected transport
/// ([replication backend-link L2 §6](../replication/backend-link/L2.md), assertion `transport-selected-by-config`).
/// The co-located default wraps the in-process far node; `Remote` wraps a
/// [`RemoteBackend`] pointed at a backend serving the link wire. An override
/// decorator, when present, composes over that real transport (delegating what
/// it does not intercept) — it does not replace the surface.
fn select_backend_link(
    transport: &BackendTransportConfig,
    override_decorator: Option<BackendTransportDecorator>,
    backend: Arc<Backend>,
) -> BackendLink {
    let base: Arc<dyn BackendApi> = match transport {
        BackendTransportConfig::InProcess => Arc::new(LocalBackend::new(backend)),
        BackendTransportConfig::Remote { base_url, token } => {
            Arc::new(RemoteBackend::with_token(base_url.clone(), token.clone()))
        }
    };
    match override_decorator {
        Some(decorate) => BackendLink::new(decorate(base)),
        None => BackendLink::new(base),
    }
}

/// A runtime handle built from pre-existing api parts, plus the backend's
/// account-mutation service for the OAuth holdout.
///
/// spec: docs/eph/PLAN-L3-api-runtime-wrapper-migration#appstate-has-runtime-handle
pub struct MigrationRuntime {
    pub handle: RuntimeHandle,
    pub account_mutations: Arc<AccountMutationService>,
}

/// MIGRATION(api-runtime-wrapper): build a runtime handle around existing
/// test/API parts until all router state is produced by the runtime builder.
pub fn from_api_bridge_for_migration(
    api_bridge: AuthorityRuntimeApiMigrationBridge,
    account_count: usize,
) -> RuntimeHandle {
    migration_runtime(
        api_bridge,
        account_count,
        Arc::new(DefaultAccountRuntimeOverviewProvider),
        Arc::new(UnavailableLiveAccountRuntimeProvider),
        None,
    )
    .0
}

pub fn from_api_bridge_with_status_provider_for_migration(
    api_bridge: AuthorityRuntimeApiMigrationBridge,
    account_count: usize,
    status_provider: Arc<dyn AccountRuntimeOverviewProvider>,
) -> RuntimeHandle {
    migration_runtime(
        api_bridge,
        account_count,
        status_provider,
        Arc::new(UnavailableLiveAccountRuntimeProvider),
        None,
    )
    .0
}

pub fn from_api_bridge_with_providers_for_migration(
    api_bridge: AuthorityRuntimeApiMigrationBridge,
    account_count: usize,
    status_provider: Arc<dyn AccountRuntimeOverviewProvider>,
    live_accounts: Arc<dyn LiveAccountRuntimeProvider>,
) -> RuntimeHandle {
    migration_runtime(
        api_bridge,
        account_count,
        status_provider,
        live_accounts,
        None,
    )
    .0
}

pub fn from_api_bridge_with_account_supervisor_for_migration(
    api_bridge: AuthorityRuntimeApiMigrationBridge,
    account_count: usize,
    account_supervisor: Arc<AccountSupervisor>,
) -> MigrationRuntime {
    let (handle, account_mutations) = migration_runtime(
        api_bridge,
        account_count,
        account_supervisor.clone(),
        account_supervisor.clone(),
        Some(account_supervisor),
    );
    MigrationRuntime {
        handle,
        account_mutations: account_mutations
            .expect("the account-supervisor variant always builds the mutation service"),
    }
}

fn migration_runtime(
    api_bridge: AuthorityRuntimeApiMigrationBridge,
    account_count: usize,
    status_provider: Arc<dyn AccountRuntimeOverviewProvider>,
    live_accounts: Arc<dyn LiveAccountRuntimeProvider>,
    account_supervisor: Option<Arc<AccountSupervisor>>,
) -> (RuntimeHandle, Option<Arc<AccountMutationService>>) {
    let account_reads = Arc::new(AccountReadService::new(
        api_bridge.service.clone(),
        status_provider,
    ));
    let query_supervisor = account_supervisor.clone().unwrap_or_else(|| {
        Arc::new(AccountSupervisor::new(
            api_bridge.service.clone(),
            api_bridge.store.clone(),
            api_bridge.secret_store.clone(),
            api_bridge.event_sender.clone(),
            Duration::from_secs(60),
        ))
    });
    let account_mutations = account_supervisor.map(|supervisor| {
        let account_repository = Arc::new(AccountRepository::new(
            api_bridge.service.clone(),
            api_bridge.secret_store.clone(),
        ));
        Arc::new(AccountMutationService::new(
            api_bridge.service.clone(),
            api_bridge.store.clone(),
            account_repository,
            api_bridge.event_sender.clone(),
            supervisor,
            account_reads.clone(),
        ))
    });
    let mail_queries = Arc::new(MailQueryService::new(
        api_bridge.service.clone(),
        query_supervisor,
    ));
    let backend = Arc::new(Backend::new(
        api_bridge.service.clone(),
        api_bridge.store.clone(),
        mail_queries,
        account_reads,
        account_mutations.clone(),
        live_accounts.clone(),
        api_bridge.event_sender.clone(),
    ));
    let reads = Arc::new(ReadCache::passthrough(Arc::new(LocalBackend::new(
        backend.clone(),
    ))));
    let runtime_status = RuntimeStatus {
        lifecycle: RuntimeLifecycle::Ready,
        store: RuntimeStoreStatus {
            config_loaded: true,
            state_store_open: true,
            cache_root_ready: false,
        },
        account_count,
    };
    let backend_link = BackendLink::new(Arc::new(LocalBackend::new(backend)));
    let composed = assemble_runtime(RuntimeAssembly {
        backend_link,
        reads,
        event_sender: api_bridge.event_sender,
        startup_status: runtime_status,
        drive_down_channel: false,
    });
    (composed.handle, account_mutations)
}
