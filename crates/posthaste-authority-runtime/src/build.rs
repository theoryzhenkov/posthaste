use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    AccountId, AddToMailboxCommand, AppSettings, ConfigError, ConfigRepository, DomainEvent,
    EventFilter, MailService, MailStore, MailboxId, MailboxSummary, MessageId, Operation,
    OperationId, RemoveFromMailboxCommand, ReplaceMailboxesCommand, SecretStore,
    SendMessageRequest, ServiceError, SetKeywordsCommand, SmartMailboxId,
    StoreError, SyncMode,
};
use posthaste_link_contract::BackendLink;
use posthaste_observability::{events, ph_warn};
use posthaste_runtime_contract::{
    AccountScopeRequest, AccountVerificationResult, CreateAccountMutation, MailQueryPage,
    MailQueryRequest, MessageResourceKind, MutationReceipt, MutationRequest,
    MutationSettlementState, PatchAccountMutation, RuntimeAccountList, RuntimeCaller, RuntimeCore,
    RuntimeError, RuntimeErrorCode, RuntimeEventSubscription, RuntimeFrameSubscription,
    RuntimeLifecycle, RuntimeResourceBytes, RuntimeSession, RuntimeSessionId, RuntimeSessionSeq,
    RuntimeStatus, RuntimeStoreStatus, RuntimeViewSubscription, ViewDescriptor, ViewId,
    ViewRevision,
};
use posthaste_store::DatabaseStore;
use thiserror::Error;
use tokio::sync::broadcast;

use crate::account_reads::{AccountReadService, DefaultAccountRuntimeOverviewProvider};
use crate::account_repository::AccountRepository;
use crate::backend::{
    keyword_toggle, parse_args, Backend, MessageMoveToMailboxArgs, MessageMoveToRoleArgs,
    MessageReplaceMailboxesArgs, MessageSetFlaggedStateArgs, MessageSetKeywordsMutationArgs,
    MessageSetReadStateArgs, MessageSetUserTagsArgs, MessageTargetArgs,
};
use crate::near_node::{named_message_assertion, RuntimeBackendOutbox};
use crate::read::ReadCache;
use crate::transport::{LocalBackend, RemoteBackend};
use posthaste_link_contract::BackendApi;
use posthaste_link_core::{MutationId, PendingMessageMutation};
use crate::bootstrap::initialize_config;
use crate::mail_queries::MailQueryService;
use crate::mutations::AccountMutationService;
use crate::oauth::{OAuthExchangeResult, OAuthProviderProfile, OAuthTokenSet};
use crate::sessions::{HistoryRecord, MutationAcceptance, MutationCommand, SessionRegistry};
use crate::views::ViewRegistry;
use crate::{
    AccountRuntimeOverviewProvider, AccountSupervisor, LiveAccountRuntimeProvider,
    SystemSecretStore, UnavailableLiveAccountRuntimeProvider,
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
    pub poll_interval: Duration,
    /// Which transport carries the runtime↔backend link ([replication L4 §5](../replication/L4.md)).
    /// Chosen from configuration, not at build time; the default is in-process
    /// co-located (assertion `transport-selected-by-config`).
    pub backend_transport: BackendTransportConfig,
    /// A pre-built link transport that overrides [`backend_transport`](Self::backend_transport)
    /// when set. A host/test seam for injecting a custom transport (e.g. a
    /// deferred one to exercise the near-node outbox); `None` in normal builds.
    pub backend_transport_override: Option<Arc<dyn BackendApi>>,
}

/// The runtime↔backend link transport, selected by configuration.
///
/// `InProcess` (default) is the co-located far node — zero serialization, byte
/// for byte the pre-link behavior. `Remote` points the link at a backend that
/// serves the link wire (POST up + SSE down) elsewhere; switching is a config
/// change, not a rebuild ([replication L4 §5](../replication/L4.md)).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BackendTransportConfig {
    #[default]
    InProcess,
    Remote {
        base_url: String,
    },
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
            poll_interval: Duration::from_secs(60),
            backend_transport: BackendTransportConfig::InProcess,
            backend_transport_override: None,
        }
    }

    /// Select the runtime↔backend link transport (default in-process).
    pub fn with_backend_transport(mut self, backend_transport: BackendTransportConfig) -> Self {
        self.backend_transport = backend_transport;
        self
    }

    /// Inject a pre-built link transport, overriding [`backend_transport`](Self::backend_transport).
    pub fn with_backend_transport_override(
        mut self,
        transport: Arc<dyn BackendApi>,
    ) -> Self {
        self.backend_transport_override = Some(transport);
        self
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

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
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
    pub account_supervisor: Arc<AccountSupervisor>,
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
    /// @spec docs/replication/L4#3-the-link-contract-backendlink
    pub backend_link: BackendLink,
}

/// Temporary bridge for compatibility harnesses and migration handle constructors
/// while direct service/store access is retired from API route state.
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
/// The bundled/daemon composition ([replication L5 §4](../replication/L5.md)):
/// build the backend far node, then the runtime near node over it (in-process
/// link by default). Behavior-preserving — the same graph as before, factored so
/// the two roles can also be composed independently by the lean binaries.
///
/// spec: docs/eph/PLAN-L2-bundled-app-test-plan#authority-runtime-handle-test-first
/// spec: docs/runtime/L2#runtime-builder-transport-free
pub async fn build_authority_runtime(
    config: AuthorityRuntimeBuildConfig,
) -> Result<AuthorityRuntimeBuild, AuthorityRuntimeBuildError> {
    let backend = build_backend(&config).await?;
    Ok(build_runtime(backend, &config))
}

/// The backend far-node graph: store + service + providers (the account
/// supervisor) + the L4 [`Backend`], built with no runtime near node. The
/// `posthaste-backend` binary serves this over the link; the bundled/daemon
/// compositions hand it to [`build_runtime`]
/// ([replication L5 §4](../replication/L5.md), assertion `backend-builds-standalone`).
pub(crate) struct BackendBuild {
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    secret_store: Arc<dyn SecretStore>,
    event_sender: broadcast::Sender<DomainEvent>,
    account_supervisor: Arc<AccountSupervisor>,
    account_reads: Arc<AccountReadService>,
    account_mutations: Arc<AccountMutationService>,
    backend: Arc<Backend>,
    api_bridge: AuthorityRuntimeApiMigrationBridge,
    runtime_status: RuntimeStatus,
}

/// Build the backend far node alone (no runtime). Used directly by a
/// backend-only deployment and as the first half of the bundled build.
pub(crate) async fn build_backend(
    config: &AuthorityRuntimeBuildConfig,
) -> Result<BackendBuild, AuthorityRuntimeBuildError> {
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
        account_reads.clone(),
        account_supervisor.clone(),
        event_sender.clone(),
    ));

    Ok(BackendBuild {
        service,
        store,
        secret_store,
        event_sender,
        account_supervisor,
        account_reads,
        account_mutations,
        backend,
        api_bridge,
        runtime_status,
    })
}

/// Build the runtime near node over a backend. The bundled/daemon composition
/// passes the in-process [`BackendBuild`]; the link is selected from config
/// (in-process over that backend, or remote). Must run within a Tokio runtime
/// (it may spawn the cache-coherence task for a split runtime).
pub(crate) fn build_runtime(
    backend: BackendBuild,
    config: &AuthorityRuntimeBuildConfig,
) -> AuthorityRuntimeBuild {
    let BackendBuild {
        service,
        store,
        secret_store,
        event_sender,
        account_supervisor,
        account_reads,
        account_mutations,
        backend,
        api_bridge,
        runtime_status,
    } = backend;

    let stopped = Arc::new(AtomicBool::new(false));
    let outbox = Arc::new(RuntimeBackendOutbox::new());
    let backend_link = select_backend_link(
        &config.backend_transport,
        config.backend_transport_override.clone(),
        backend.clone(),
    );
    let reads = Arc::new(build_read_cache(
        &config.backend_transport,
        &backend,
        &backend_link,
    ));
    // A split runtime's retaining cache is kept coherent by the down-channel.
    if matches!(config.backend_transport, BackendTransportConfig::Remote { .. }) {
        tokio::spawn(crate::read::run_cache_coherence(
            backend_link.clone(),
            reads.clone(),
        ));
    }
    let views = Arc::new(ViewRegistry::new(
        account_reads,
        event_sender.clone(),
        outbox.clone(),
        reads.clone(),
    ));
    let sessions = Arc::new(SessionRegistry::new(views.clone(), event_sender.clone()));
    let core = Arc::new(AuthorityRuntimeCore {
        service,
        store,
        backend,
        backend_link: backend_link.clone(),
        outbox,
        reads,
        event_sender,
        account_mutations: Some(account_mutations),
        views,
        sessions,
        live_accounts: account_supervisor.clone(),
        startup_status: runtime_status.clone(),
        stopped: stopped.clone(),
    });

    AuthorityRuntimeBuild {
        handle: AuthorityRuntimeHandle { core },
        shutdown: RuntimeShutdownHandle { stopped },
        runtime_status,
        account_supervisor,
        secret_store,
        api_bridge,
        backend_link,
    }
}

/// Build the runtime's read-through cache from the same config that picks the
/// transport. `Remote` reads through the link and **retains** what flows back
/// (kept coherent by the down-channel); `InProcess` reads the co-located far
/// node directly with a **passthrough** policy (no redundant cache, instant
/// link). The transport *override* (a write/link test seam) does not change the
/// read path.
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
/// ([replication L4 §5](../replication/L4.md), assertion `transport-selected-by-config`).
/// The co-located default wraps the in-process far node; `Remote` wraps a
/// [`RemoteBackend`] pointed at a backend serving the link wire.
fn select_backend_link(
    transport: &BackendTransportConfig,
    override_transport: Option<Arc<dyn BackendApi>>,
    backend: Arc<Backend>,
) -> BackendLink {
    if let Some(transport) = override_transport {
        return BackendLink::new(transport);
    }
    match transport {
        BackendTransportConfig::InProcess => {
            BackendLink::new(Arc::new(LocalBackend::new(backend)))
        }
        BackendTransportConfig::Remote { base_url } => {
            BackendLink::new(Arc::new(RemoteBackend::new(base_url.clone())))
        }
    }
}

struct AuthorityRuntimeCore {
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    // account_reads is not held here: account/config reads route through `reads`
    // (the BackendApi read path); ViewRegistry keeps its own for AccountStatus.
    /// The backend far node — the single owner of message-command backend
    /// access (L4 W1). Reached in-process via [`backend_link`](Self::backend_link).
    backend: Arc<Backend>,
    /// The runtime↔backend link over its (config-selected, in-process by
    /// default) transport. The mutation up-channel goes through here.
    backend_link: BackendLink,
    /// The runtime's outbox toward the backend: forwarded-but-unconfirmed
    /// mutations, folded optimistically into served views (L4 §4.3).
    outbox: Arc<RuntimeBackendOutbox>,
    /// The read-through cache over the far node (W4a: passthrough). Point reads
    /// and the mail-list base draw from here.
    reads: Arc<ReadCache>,
    event_sender: broadcast::Sender<DomainEvent>,
    account_mutations: Option<Arc<AccountMutationService>>,
    views: Arc<ViewRegistry>,
    sessions: Arc<SessionRegistry>,
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
        Self::from_api_bridge_with_optional_mutations_for_migration(
            api_bridge,
            account_count,
            status_provider,
            live_accounts,
            None,
        )
    }

    pub fn from_api_bridge_with_account_supervisor_for_migration(
        api_bridge: AuthorityRuntimeApiMigrationBridge,
        account_count: usize,
        account_supervisor: Arc<crate::AccountSupervisor>,
    ) -> Self {
        Self::from_api_bridge_with_optional_mutations_for_migration(
            api_bridge,
            account_count,
            account_supervisor.clone(),
            account_supervisor.clone(),
            Some(account_supervisor),
        )
    }

    fn from_api_bridge_with_optional_mutations_for_migration(
        api_bridge: AuthorityRuntimeApiMigrationBridge,
        account_count: usize,
        status_provider: Arc<dyn AccountRuntimeOverviewProvider>,
        live_accounts: Arc<dyn LiveAccountRuntimeProvider>,
        account_supervisor: Option<Arc<crate::AccountSupervisor>>,
    ) -> Self {
        let account_reads = Arc::new(AccountReadService::new(
            api_bridge.service.clone(),
            status_provider,
        ));
        let query_supervisor = account_supervisor.clone().unwrap_or_else(|| {
            Arc::new(crate::AccountSupervisor::new(
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
        let outbox = Arc::new(RuntimeBackendOutbox::new());
        let backend = Arc::new(Backend::new(
            api_bridge.service.clone(),
            api_bridge.store.clone(),
            mail_queries,
            account_reads.clone(),
            live_accounts.clone(),
            api_bridge.event_sender.clone(),
        ));
        let reads = Arc::new(ReadCache::passthrough(Arc::new(LocalBackend::new(
            backend.clone(),
        ))));
        let views = Arc::new(ViewRegistry::new(
            account_reads,
            api_bridge.event_sender.clone(),
            outbox.clone(),
            reads.clone(),
        ));
        let sessions = Arc::new(SessionRegistry::new(
            views.clone(),
            api_bridge.event_sender.clone(),
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
        let backend_link = BackendLink::new(Arc::new(LocalBackend::new(backend.clone())));
        Self {
            core: Arc::new(AuthorityRuntimeCore {
                service: api_bridge.service.clone(),
                store: api_bridge.store.clone(),
                backend,
                backend_link,
                outbox,
                reads,
                event_sender: api_bridge.event_sender.clone(),
                account_mutations,
                views,
                sessions,
                live_accounts,
                startup_status: runtime_status,
                stopped: Arc::new(AtomicBool::new(false)),
            }),
        }
    }

    fn current_status(&self) -> RuntimeStatus {
        let mut status = self.core.startup_status.clone();
        if let Some(account_count) = self.core.live_accounts.account_count() {
            status.account_count = account_count;
        }
        if self.core.stopped.load(Ordering::SeqCst) {
            status.lifecycle = RuntimeLifecycle::Stopped;
        }
        status
    }

    fn ensure_runtime_active(&self) -> Result<(), RuntimeError> {
        let lifecycle = self.current_status().lifecycle;
        if matches!(
            lifecycle,
            RuntimeLifecycle::Ready | RuntimeLifecycle::Degraded
        ) {
            return Ok(());
        }
        let message = format!("runtime is {}", runtime_lifecycle_label(&lifecycle));
        Err(RuntimeError::with_details(
            RuntimeErrorCode::RuntimeNotReady,
            message,
            serde_json::json!({ "lifecycle": lifecycle }),
        ))
    }

    fn account_mutations(&self) -> Result<Arc<AccountMutationService>, RuntimeError> {
        self.core.account_mutations.clone().ok_or_else(|| {
            RuntimeError::runtime_not_ready("account mutation runtime is not available")
        })
    }

    /// Nudge the account to sync so just-enqueued outbox operations flush
    /// promptly. Delegates to the backend far node, which owns the supervisor
    /// handle; the non-command paths (send/draft) still flush through here.
    async fn trigger_outbox_flush(&self, account_id: &AccountId) {
        self.core.backend.trigger_outbox_flush(account_id).await;
    }

    pub async fn create_oauth_account_from_exchange(
        &self,
        profile: &OAuthProviderProfile,
        exchange: OAuthExchangeResult,
    ) -> Result<posthaste_domain::AccountOverview, RuntimeError> {
        self.ensure_runtime_active()?;
        self.account_mutations()?
            .create_oauth_account_from_exchange(profile, exchange)
            .await
    }

    pub async fn persist_oauth_token_set(
        &self,
        account_id: AccountId,
        token_set: OAuthTokenSet,
    ) -> Result<posthaste_domain::AccountOverview, RuntimeError> {
        self.ensure_runtime_active()?;
        self.account_mutations()?
            .persist_oauth_token_set(account_id, token_set)
            .await
    }

    fn ensure_account_in_scope(
        account_id: &str,
        account_scope: Option<&[String]>,
    ) -> Result<(), RuntimeError> {
        if account_scope.is_some_and(|scope| !scope.iter().any(|id| id == account_id)) {
            return Err(RuntimeError::unauthorized(
                "mutation source is outside the runtime session account scope",
            ));
        }
        Ok(())
    }

    /// Accept a named message mutation (idempotency), run its command, and
    /// settle confirmed/failed on the session stream. The shared
    /// accept -> execute -> settle flow for every message mutation; the command
    /// `action` is one of the existing handle methods, which already publishes
    /// the optimistic assertion and flushes the outbox.
    ///
    /// @spec docs/runtime/L2#mutation-pipeline-and-catalog
    /// Accept a named message mutation onto the session (idempotency + history),
    /// forward it up the backend link, and settle the session stream from the
    /// backend's receipt. The `forward` future is the link's up-channel
    /// (`BackendLink::forward_mutation`); its receipt carries the command's
    /// events as `output` and the backend's confirmation id. Scope and history
    /// are the runtime's (near-node) concern and are resolved before this call.
    async fn run_message_mutation<Fut>(
        &self,
        caller: RuntimeCaller,
        request: &MutationRequest,
        history: HistoryRecord,
        forward: Fut,
    ) -> Result<MutationReceipt, RuntimeError>
    where
        Fut: std::future::Future<Output = Result<MutationReceipt, RuntimeError>>,
    {
        let session_id = request.session_id.clone().ok_or_else(|| {
            RuntimeError::invalid_mutation("runtime mutation requires a session id")
        })?;
        let mutation_id = match self
            .core
            .sessions
            .accept_mutation(caller, request, history)?
        {
            MutationAcceptance::New { mutation_id, .. } => mutation_id,
            MutationAcceptance::Existing(receipt) => return Ok(receipt),
        };
        match forward.await {
            Ok(backend_receipt) => {
                // The backend already serialized the command's events as the
                // receipt output (state-before-event: the effect is applied
                // before the receipt returns); settle the session with it.
                self.core.sessions.settle_mutation(
                    &session_id,
                    &mutation_id,
                    MutationSettlementState::Confirmed,
                    None,
                    backend_receipt.output,
                )
            }
            Err(error) => {
                let envelope = error.envelope().clone();
                self.core.sessions.settle_mutation(
                    &session_id,
                    &mutation_id,
                    MutationSettlementState::Failed,
                    Some(envelope),
                    serde_json::Value::Null,
                )
            }
        }
    }

    /// Read the message's current overlay-folded summary (keywords + mailbox
    /// membership) without provider work, for computing an undo inverse.
    async fn current_message_summary(
        &self,
        source_id: &str,
        message_id: &str,
    ) -> Result<Option<posthaste_domain::MessageSummary>, RuntimeError> {
        // Read through the far node (W4a passthrough; W4c serves from cache or
        // reads through over the link). This is the c3 split-runtime read.
        self.core
            .reads
            .current_summary(
                &AccountId(source_id.to_string()),
                &MessageId(message_id.to_string()),
            )
            .await
    }

    /// The precise keyword command that restores the message's current keyword
    /// set after `command` is applied. Reads current keywords so the inverse is
    /// correct even when the forward command is a partial no-op (e.g. adding a
    /// keyword that was already present).
    ///
    /// @spec docs/runtime/L2#mutation-pipeline-and-catalog
    async fn keyword_inverse(
        &self,
        source_id: &str,
        message_id: &str,
        command: &SetKeywordsCommand,
    ) -> Result<MutationCommand, RuntimeError> {
        let present: std::collections::HashSet<String> = self
            .current_message_summary(source_id, message_id)
            .await?
            .map(|summary| summary.keywords)
            .unwrap_or_default()
            .into_iter()
            .collect();
        // Re-add keywords that were present and would be removed; remove keywords
        // that were absent and would be added. Untouched keywords stay as-is.
        let add: Vec<String> = command
            .remove
            .iter()
            .filter(|keyword| present.contains(*keyword))
            .cloned()
            .collect();
        let remove: Vec<String> = command
            .add
            .iter()
            .filter(|keyword| !present.contains(*keyword))
            .cloned()
            .collect();
        Ok(MutationCommand {
            name: "message.setKeywords".to_string(),
            args: serde_json::json!({
                "sourceId": source_id,
                "messageId": message_id,
                "command": { "add": add, "remove": remove },
            }),
        })
    }

    /// The `replaceMailboxes` command that restores the message's current mailbox
    /// membership. `None` when the message can't be read, in which case the
    /// mutation is treated as non-invertible.
    ///
    /// @spec docs/runtime/L2#mutation-pipeline-and-catalog
    async fn mailbox_inverse(
        &self,
        source_id: &str,
        message_id: &str,
    ) -> Result<Option<MutationCommand>, RuntimeError> {
        let Some(summary) = self.current_message_summary(source_id, message_id).await? else {
            return Ok(None);
        };
        Ok(Some(MutationCommand {
            name: "message.replaceMailboxes".to_string(),
            args: serde_json::json!({
                "sourceId": source_id,
                "messageId": message_id,
                "mailboxIds": summary.mailbox_ids,
            }),
        }))
    }

    fn event_matches_filter(event: &DomainEvent, filter: &EventFilter) -> bool {
        if let Some(account_id) = &filter.account_id {
            if &event.account_id != account_id {
                return false;
            }
        }
        if let Some(after_seq) = filter.after_seq {
            if event.seq <= after_seq {
                return false;
            }
        }
        if let Some(topic) = &filter.topic {
            if &event.topic != topic {
                return false;
            }
        }
        if let Some(mailbox_id) = &filter.mailbox_id {
            if event.mailbox_id.as_ref() != Some(mailbox_id) {
                return false;
            }
        }
        true
    }

    fn live_event_stream(
        mut receiver: broadcast::Receiver<DomainEvent>,
        filter: EventFilter,
        replayed_through: Option<i64>,
    ) -> posthaste_runtime_contract::RuntimeEventStream {
        let stream = async_stream::stream! {
            loop {
                match receiver.recv().await {
                    Ok(event)
                        if replayed_through.is_none_or(|seq| event.seq > seq)
                            && Self::event_matches_filter(&event, &filter) =>
                    {
                        yield event;
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };
        stream.boxed()
    }

    /// Route a single named message mutation to its handle action (which
    /// enqueues the outbox op, publishes the optimistic assertion, and flushes)
    /// wrapped in the shared accept -> execute -> settle flow. `record` is true
    /// for fresh user actions (which capture an inverse onto the undo stack) and
    /// false for the replays driven by undo/redo.
    ///
    /// @spec docs/runtime/L2#mutation-pipeline-and-catalog
    async fn dispatch_named_mutation(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        request: MutationRequest,
        record: bool,
    ) -> Result<MutationReceipt, RuntimeError> {
        let session_scope = self
            .core
            .sessions
            .session_scope(&session_id, caller.account_scope.as_deref())?;
        // Runtime (near-node) concerns: scope enforcement and undo-history
        // capture, both per mutation. The command application itself is the
        // backend's; it is forwarded up the link below, uniform across names.
        let history = match request.name.as_str() {
            "message.setKeywords" => {
                let args: MessageSetKeywordsMutationArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                self.keyword_history(record, &args.source_id, &args.message_id, &args.command)
                    .await?
            }
            "message.setReadState" => {
                let args: MessageSetReadStateArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                let command = keyword_toggle("$seen", args.read);
                self.keyword_history(record, &args.source_id, &args.message_id, &command)
                    .await?
            }
            "message.setFlaggedState" => {
                let args: MessageSetFlaggedStateArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                let command = keyword_toggle("$flagged", args.flagged);
                self.keyword_history(record, &args.source_id, &args.message_id, &command)
                    .await?
            }
            "message.setUserTags" => {
                let args: MessageSetUserTagsArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                let command = SetKeywordsCommand {
                    add: args.add,
                    remove: args.remove,
                };
                self.keyword_history(record, &args.source_id, &args.message_id, &command)
                    .await?
            }
            "message.moveToMailbox" => {
                let args: MessageMoveToMailboxArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                self.mailbox_history(record, &args.source_id, &args.message_id)
                    .await?
            }
            "message.replaceMailboxes" => {
                let args: MessageReplaceMailboxesArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                self.mailbox_history(record, &args.source_id, &args.message_id)
                    .await?
            }
            "message.moveToRole" => {
                let args: MessageMoveToRoleArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                self.mailbox_history(record, &args.source_id, &args.message_id)
                    .await?
            }
            "message.archive" | "message.trash" | "message.restoreToInbox" => {
                let args: MessageTargetArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                self.mailbox_history(record, &args.source_id, &args.message_id)
                    .await?
            }
            "message.destroy" => {
                let args: MessageTargetArgs = parse_args(&request)?;
                Self::ensure_account_in_scope(&args.source_id, session_scope.as_deref())?;
                // Destroy is the one non-invertible message mutation.
                HistoryRecord::Skip
            }
            _ => {
                return Err(RuntimeError::invalid_mutation(format!(
                    "unknown runtime mutation '{}'",
                    request.name
                )))
            }
        };
        // Accept the mutation into the runtime's outbox toward the backend so
        // recomputed views fold it optimistically while it is in flight; retire
        // it once the backend confirms (or fails). In the in-process default the
        // forward confirms synchronously, so the outbox is empty between
        // mutations and the overlay is a pass-through (`colocated-unchanged`).
        let optimistic = named_message_assertion(&request).map(|(message_id, assertion)| {
            let id = MutationId(request.client_mutation_id.as_str().to_string());
            self.core.outbox.accept(PendingMessageMutation {
                id: id.clone(),
                message_id,
                assertion,
            });
            id
        });
        // Up-channel: forward the named mutation to the backend far node.
        let forward = self.core.backend_link.forward_mutation(request.clone());
        let result = self
            .run_message_mutation(caller, &request, history, forward)
            .await;
        if let Some(id) = optimistic {
            self.core.outbox.retire(&id);
        }
        result
    }

    /// History plan for a keyword mutation: capture the inverse when this is a
    /// fresh user action, otherwise skip (the replay driven by undo/redo).
    async fn keyword_history(
        &self,
        record: bool,
        source_id: &str,
        message_id: &str,
        command: &SetKeywordsCommand,
    ) -> Result<HistoryRecord, RuntimeError> {
        if !record {
            return Ok(HistoryRecord::Skip);
        }
        Ok(HistoryRecord::Record(
            self.keyword_inverse(source_id, message_id, command).await?,
        ))
    }

    /// History plan for a mailbox mutation. Skips recording for undo/redo
    /// replays and when the message can't be read (non-invertible).
    async fn mailbox_history(
        &self,
        record: bool,
        source_id: &str,
        message_id: &str,
    ) -> Result<HistoryRecord, RuntimeError> {
        if !record {
            return Ok(HistoryRecord::Skip);
        }
        Ok(match self.mailbox_inverse(source_id, message_id).await? {
            Some(inverse) => HistoryRecord::Record(inverse),
            None => HistoryRecord::Skip,
        })
    }

    /// Reverse the most recent reversible mutation by replaying its captured
    /// inverse, then make the step redoable. Errors when there is nothing to
    /// undo.
    ///
    /// @spec docs/runtime/L2#mutation-pipeline-and-catalog
    async fn run_undo(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        request: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        let Some(entry) = self.core.sessions.pop_undo(&session_id)? else {
            return Err(RuntimeError::invalid_mutation("nothing to undo"));
        };
        let replay = MutationRequest {
            session_id: Some(session_id.clone()),
            name: entry.inverse.name.clone(),
            args: entry.inverse.args.clone(),
            client_mutation_id: request.client_mutation_id,
            context: request.context,
        };
        match self
            .dispatch_named_mutation(caller, session_id.clone(), replay, false)
            .await
        {
            Ok(receipt) => {
                self.core.sessions.push_redo(&session_id, entry)?;
                self.core.sessions.emit_history_frame(&session_id)?;
                Ok(receipt)
            }
            Err(error) => {
                // The replay failed before it took effect; keep the step undoable.
                self.core.sessions.restore_undo(&session_id, entry)?;
                Err(error)
            }
        }
    }

    /// Re-apply the most recently undone mutation by replaying its forward
    /// command, then make it undoable again. Errors when there is nothing to
    /// redo.
    ///
    /// @spec docs/runtime/L2#mutation-pipeline-and-catalog
    async fn run_redo(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        request: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        let Some(entry) = self.core.sessions.pop_redo(&session_id)? else {
            return Err(RuntimeError::invalid_mutation("nothing to redo"));
        };
        let replay = MutationRequest {
            session_id: Some(session_id.clone()),
            name: entry.forward.name.clone(),
            args: entry.forward.args.clone(),
            client_mutation_id: request.client_mutation_id,
            context: request.context,
        };
        match self
            .dispatch_named_mutation(caller, session_id.clone(), replay, false)
            .await
        {
            Ok(receipt) => {
                self.core.sessions.restore_undo(&session_id, entry)?;
                self.core.sessions.emit_history_frame(&session_id)?;
                Ok(receipt)
            }
            Err(error) => {
                // The replay failed before it took effect; keep the step redoable.
                self.core.sessions.push_redo(&session_id, entry)?;
                Err(error)
            }
        }
    }
}

fn runtime_lifecycle_label(lifecycle: &RuntimeLifecycle) -> &'static str {
    match lifecycle {
        RuntimeLifecycle::Starting => "starting",
        RuntimeLifecycle::Ready => "ready",
        RuntimeLifecycle::Degraded => "degraded",
        RuntimeLifecycle::Stopping => "stopping",
        RuntimeLifecycle::Stopped => "stopped",
    }
}

#[async_trait]
impl RuntimeCore for AuthorityRuntimeHandle {
    async fn runtime_status(&self, _caller: RuntimeCaller) -> Result<RuntimeStatus, RuntimeError> {
        Ok(self.current_status())
    }

    async fn get_app_settings(&self, _caller: RuntimeCaller) -> Result<AppSettings, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.app_settings().await
    }

    async fn patch_app_settings(
        &self,
        _caller: RuntimeCaller,
        mutation: posthaste_runtime_contract::PatchAppSettingsMutation,
    ) -> Result<AppSettings, RuntimeError> {
        self.ensure_runtime_active()?;
        self.account_mutations()?.patch_app_settings(mutation)
    }

    async fn preview_automation_rule(
        &self,
        _caller: RuntimeCaller,
        mutation: posthaste_runtime_contract::AutomationRulePreviewMutation,
    ) -> Result<posthaste_runtime_contract::AutomationRulePreviewResult, RuntimeError> {
        self.ensure_runtime_active()?;
        self.account_mutations()?.preview_automation_rule(mutation)
    }

    async fn list_accounts(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<RuntimeAccountList, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_accounts().await
    }

    async fn get_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<posthaste_domain::AccountOverview, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .reads
            .get_account(account_id)
            .await?
            .ok_or_else(|| RuntimeError::not_found("account not found"))
    }

    async fn resolve_account_scope(
        &self,
        _caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<Vec<AccountId>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.resolve_account_scope(scope).await
    }

    async fn list_mailboxes(
        &self,
        _caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<
        std::collections::BTreeMap<AccountId, Vec<posthaste_domain::MailboxSummary>>,
        RuntimeError,
    > {
        self.ensure_runtime_active()?;
        self.core.reads.list_mailboxes(scope).await
    }

    async fn list_smart_mailboxes(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<posthaste_domain::SmartMailboxSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_smart_mailboxes().await
    }

    async fn get_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<posthaste_domain::SmartMailbox, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.get_smart_mailbox(smart_mailbox_id).await
    }

    async fn create_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        mutation: posthaste_runtime_contract::CreateSmartMailboxMutation,
    ) -> Result<posthaste_domain::SmartMailbox, RuntimeError> {
        self.ensure_runtime_active()?;
        self.account_mutations()?.create_smart_mailbox(mutation)
    }

    async fn patch_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
        mutation: posthaste_runtime_contract::PatchSmartMailboxMutation,
    ) -> Result<posthaste_domain::SmartMailbox, RuntimeError> {
        self.ensure_runtime_active()?;
        self.account_mutations()?
            .patch_smart_mailbox(smart_mailbox_id, mutation)
    }

    async fn delete_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.account_mutations()?
            .delete_smart_mailbox(smart_mailbox_id)
    }

    async fn reset_default_smart_mailboxes(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<posthaste_domain::SmartMailboxSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.account_mutations()?.reset_default_smart_mailboxes()
    }

    async fn list_tags(
        &self,
        _caller: RuntimeCaller,
        scope: AccountScopeRequest,
    ) -> Result<Vec<posthaste_domain::TagSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_tags(scope).await
    }

    async fn get_identity(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<posthaste_domain::Identity, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.get_identity(account_id).await
    }

    async fn list_sender_addresses(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<posthaste_domain::CachedSenderAddress>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_sender_addresses().await
    }

    async fn get_reply_context(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<posthaste_domain::ReplyContext, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .reads
            .get_reply_context(account_id, message_id)
            .await
    }

    async fn query_mail_page(
        &self,
        _caller: RuntimeCaller,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.query_mail_page(request).await
    }

    async fn open_session(&self, caller: RuntimeCaller) -> Result<RuntimeSession, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.sessions.open_session(caller)
    }

    async fn subscribe_runtime_frames(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        after_seq: Option<RuntimeSessionSeq>,
    ) -> Result<RuntimeFrameSubscription, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .sessions
            .subscribe_frames(caller, session_id, after_seq)
    }

    async fn close_session(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.sessions.close_session(caller, session_id)
    }

    async fn open_session_view(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        descriptor: ViewDescriptor,
    ) -> Result<posthaste_runtime_contract::ViewSnapshot, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .sessions
            .open_view(caller, session_id, descriptor)
            .await
    }

    async fn close_session_view(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        view_id: ViewId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.sessions.close_view(caller, session_id, view_id)
    }

    async fn extend_session_view(
        &self,
        caller: RuntimeCaller,
        session_id: RuntimeSessionId,
        view_id: ViewId,
        count: usize,
    ) -> Result<posthaste_runtime_contract::ViewSnapshot, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .sessions
            .extend_view(caller, session_id, view_id, count)
            .await
    }

    async fn run_mutation(
        &self,
        caller: RuntimeCaller,
        request: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        self.ensure_runtime_active()?;
        let session_id = request.session_id.clone().ok_or_else(|| {
            RuntimeError::invalid_mutation("runtime mutation requires a session id")
        })?;
        // Undo/redo navigate the session's runtime-owned history stack; every
        // other mutation is a fresh user action that records onto it.
        //
        // @spec docs/runtime/L2#mutation-pipeline-and-catalog
        match request.name.as_str() {
            "mutation.undo" => self.run_undo(caller, session_id, request).await,
            "mutation.redo" => self.run_redo(caller, session_id, request).await,
            _ => {
                self.dispatch_named_mutation(caller, session_id, request, true)
                    .await
            }
        }
    }

    async fn open_view(
        &self,
        caller: RuntimeCaller,
        descriptor: ViewDescriptor,
    ) -> Result<posthaste_runtime_contract::ViewSnapshot, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .views
            .open_view(descriptor, caller.account_scope.as_deref())
            .await
    }

    async fn subscribe_view(
        &self,
        caller: RuntimeCaller,
        view_id: ViewId,
        after_revision: Option<ViewRevision>,
    ) -> Result<RuntimeViewSubscription, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .views
            .subscribe_view(view_id, after_revision, caller.account_scope.as_deref())
    }

    async fn send_message(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        request: SendMessageRequest,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        // Local-first: queue the send and flush it to the provider on the next
        // connectivity window; no live gateway is required to accept it.
        let sender = request.from.clone();
        self.core.service.enqueue_send(&account_id, request)?;
        if let Some(sender) = &sender {
            if let Err(error) = self.core.store.remember_sender_address(&account_id, sender) {
                ph_warn!(
                    events::SEND_SENDER_CACHE_UPDATE_FAILED,
                    source_id = %account_id,
                    sender = %sender.email,
                    error = %error,
                    "send accepted but sender address cache update failed"
                );
            }
        }
        // `trigger_outbox_flush` nudges a sync that drains the queued send (and
        // pulls the provider's Sent copy on the following cycle).
        self.trigger_outbox_flush(&account_id).await;
        Ok(())
    }

    async fn save_draft(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        draft_id: Option<MessageId>,
        request: SendMessageRequest,
    ) -> Result<Operation, RuntimeError> {
        self.ensure_runtime_active()?;
        let operation = self
            .core
            .service
            .save_draft(&account_id, draft_id, request)?;
        self.trigger_outbox_flush(&account_id).await;
        Ok(operation)
    }

    async fn delete_draft(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        draft_id: MessageId,
    ) -> Result<Operation, RuntimeError> {
        self.ensure_runtime_active()?;
        let operation = self.core.service.delete_draft(&account_id, draft_id)?;
        self.trigger_outbox_flush(&account_id).await;
        Ok(operation)
    }

    async fn list_pending_operations(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<Vec<Operation>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.list_pending_operations(account_id).await
    }

    async fn discard_operation(
        &self,
        _caller: RuntimeCaller,
        _account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.service.discard_operation(&operation_id)?;
        Ok(())
    }

    async fn retry_operation(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        operation_id: OperationId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.service.retry_operation(&operation_id)?;
        // Re-armed to pending; nudge a flush so it re-attempts promptly.
        self.trigger_outbox_flush(&account_id).await;
        Ok(())
    }

    async fn set_message_keywords(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: SetKeywordsCommand,
    ) -> Result<posthaste_domain::CommandAck, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .set_keywords(account_id, message_id, command)
            .await
    }

    async fn add_message_to_mailbox(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: AddToMailboxCommand,
    ) -> Result<posthaste_domain::CommandAck, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .add_to_mailbox(account_id, message_id, command)
            .await
    }

    async fn remove_message_from_mailbox(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: RemoveFromMailboxCommand,
    ) -> Result<posthaste_domain::CommandAck, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .remove_from_mailbox(account_id, message_id, command)
            .await
    }

    async fn replace_message_mailboxes(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: ReplaceMailboxesCommand,
    ) -> Result<posthaste_domain::CommandAck, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .replace_mailboxes(account_id, message_id, command)
            .await
    }

    async fn destroy_message(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<posthaste_domain::CommandAck, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .destroy_message(account_id, message_id)
            .await
    }

    async fn set_mailbox_role(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        mailbox_id: MailboxId,
        role: Option<String>,
    ) -> Result<Vec<MailboxSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .backend_link
            .set_mailbox_role(account_id, mailbox_id, role)
            .await
    }

    async fn get_message_detail(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<posthaste_domain::CommandResult, RuntimeError> {
        self.ensure_runtime_active()?;
        // Body-free: the detail read serves header + cached attachments only and
        // never loads the body (it is the separate `/body` lazy resource), so
        // opening a message neither provider-fetches nor materializes the body.
        let detail = self
            .core
            .reads
            .message_detail(&account_id, &message_id)
            .await?;
        Ok(posthaste_domain::CommandResult {
            detail,
            events: Vec::new(),
        })
    }

    async fn get_draft_content(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<posthaste_domain::DraftContent, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .reads
            .get_draft_content(account_id, message_id)
            .await
    }

    async fn get_message_resource(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        kind: MessageResourceKind,
    ) -> Result<RuntimeResourceBytes, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .reads
            .get_message_resource(account_id, message_id, kind)
            .await
    }

    async fn sync_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        mode: SyncMode,
    ) -> Result<usize, RuntimeError> {
        self.ensure_runtime_active()?;
        Ok(self
            .core
            .live_accounts
            .sync_account_with_mode(&account_id, mode)
            .await?)
    }

    async fn replay_events(
        &self,
        _caller: RuntimeCaller,
        filter: EventFilter,
    ) -> Result<Vec<DomainEvent>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.reads.replay_events(filter).await
    }

    async fn subscribe_events(
        &self,
        _caller: RuntimeCaller,
        filter: EventFilter,
    ) -> Result<RuntimeEventSubscription, RuntimeError> {
        self.ensure_runtime_active()?;
        let receiver = self.core.event_sender.subscribe();
        let replay = if filter.after_seq.is_some() {
            self.replay_events(RuntimeCaller::system(), filter.clone())
                .await?
                .into_iter()
                .filter(|event| Self::event_matches_filter(event, &filter))
                .collect()
        } else {
            Vec::new()
        };
        let replayed_through = replay.last().map(|event| event.seq).or(filter.after_seq);
        let live = Self::live_event_stream(receiver, filter, replayed_through);
        Ok(RuntimeEventSubscription { replay, live })
    }

    async fn create_account(
        &self,
        _caller: RuntimeCaller,
        mutation: CreateAccountMutation,
    ) -> Result<posthaste_domain::AccountOverview, RuntimeError> {
        self.ensure_runtime_active()?;
        self.account_mutations()?.create_account(mutation).await
    }

    async fn patch_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        mutation: PatchAccountMutation,
    ) -> Result<posthaste_domain::AccountOverview, RuntimeError> {
        self.ensure_runtime_active()?;
        self.account_mutations()?
            .patch_account(account_id, mutation)
            .await
    }

    async fn delete_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.account_mutations()?.delete_account(account_id).await
    }

    async fn verify_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<AccountVerificationResult, RuntimeError> {
        self.ensure_runtime_active()?;
        self.account_mutations()?.verify_account(account_id).await
    }

    async fn set_account_enabled(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        enabled: bool,
    ) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.account_mutations()?
            .set_account_enabled(account_id, enabled)
            .await
    }

    async fn reload_config(&self, _caller: RuntimeCaller) -> Result<(), RuntimeError> {
        self.ensure_runtime_active()?;
        self.account_mutations()?.reload_config().await
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
    // Async by contract: shutdown is part of the runtime's async lifecycle
    // (start/await, shutdown/await) and will await task joins as it grows.
    #[allow(clippy::unused_async)]
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
