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
    EventFilter, MailService, MailStore, MailboxId, MailboxSummary, MessageId,
    RemoveFromMailboxCommand, ReplaceMailboxesCommand, SecretStore, SendMessageRequest,
    ServiceError, ServiceErrorKind, SetKeywordsCommand, SmartMailboxId, StoreError, SyncMode,
    SyncTrigger,
};
use posthaste_observability::{events, ph_warn};
use posthaste_runtime_contract::{
    AccountScopeRequest, AccountVerificationResult, CreateAccountMutation, MailQueryPage,
    MailQueryRequest, PatchAccountMutation, RuntimeAccountList, RuntimeAttachmentBytes,
    RuntimeCaller, RuntimeCore, RuntimeError, RuntimeErrorCode, RuntimeEventSubscription,
    RuntimeFrameSubscription, RuntimeLifecycle, RuntimeSession, RuntimeSessionId,
    RuntimeSessionSeq, RuntimeStatus, RuntimeStoreStatus, RuntimeViewSubscription, ViewDescriptor,
    ViewId, ViewRevision,
};
use posthaste_store::DatabaseStore;
use thiserror::Error;
use tokio::sync::broadcast;

use crate::account_reads::{AccountReadService, DefaultAccountRuntimeOverviewProvider};
use crate::account_repository::AccountRepository;
use crate::bootstrap::initialize_config;
use crate::mail_queries::MailQueryService;
use crate::mutations::AccountMutationService;
use crate::oauth::{OAuthExchangeResult, OAuthProviderProfile, OAuthTokenSet};
use crate::sessions::SessionRegistry;
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
    let views = Arc::new(ViewRegistry::new(
        mail_queries.clone(),
        event_sender.clone(),
    ));
    let sessions = Arc::new(SessionRegistry::new(views.clone()));
    let core = Arc::new(AuthorityRuntimeCore {
        service: service.clone(),
        store: store.clone(),
        event_sender: event_sender.clone(),
        account_reads,
        account_mutations: Some(account_mutations),
        mail_queries,
        views,
        sessions,
        live_accounts: account_supervisor.clone(),
        startup_status: runtime_status.clone(),
        stopped: stopped.clone(),
    });

    Ok(AuthorityRuntimeBuild {
        handle: AuthorityRuntimeHandle { core },
        shutdown: RuntimeShutdownHandle { stopped },
        runtime_status,
        account_supervisor,
        secret_store,
        api_bridge,
    })
}

struct AuthorityRuntimeCore {
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    event_sender: broadcast::Sender<DomainEvent>,
    account_reads: Arc<AccountReadService>,
    account_mutations: Option<Arc<AccountMutationService>>,
    mail_queries: Arc<MailQueryService>,
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
    fn store_error_to_runtime_error(error: StoreError) -> RuntimeError {
        RuntimeError::new(RuntimeErrorCode::Internal, error.to_string())
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
        let views = Arc::new(ViewRegistry::new(
            mail_queries.clone(),
            api_bridge.event_sender.clone(),
        ));
        let sessions = Arc::new(SessionRegistry::new(views.clone()));
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
                service: api_bridge.service.clone(),
                store: api_bridge.store.clone(),
                event_sender: api_bridge.event_sender.clone(),
                account_reads,
                account_mutations,
                mail_queries,
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

    fn publish_events(&self, events: &[DomainEvent]) {
        for event in events {
            let _ = self.core.event_sender.send(event.clone());
        }
    }

    async fn optional_gateway(
        &self,
        account_id: &AccountId,
    ) -> Option<posthaste_domain::SharedGateway> {
        self.core.live_accounts.gateway(account_id).await.ok()
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
        Ok(self.core.account_reads.app_settings()?)
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
        Ok(self.core.account_reads.list_accounts().await?)
    }

    async fn get_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<posthaste_domain::AccountOverview, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
            .account_reads
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
        Ok(self.core.account_reads.resolve_account_scope(scope)?)
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
        self.core
            .account_reads
            .list_mailboxes(scope)
            .map_err(|error| {
                if error.kind() == ServiceErrorKind::NotFound {
                    RuntimeError::with_details(
                        RuntimeErrorCode::NotFound,
                        "account not found",
                        serde_json::json!({}),
                    )
                } else {
                    error.into()
                }
            })
    }

    async fn list_smart_mailboxes(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<posthaste_domain::SmartMailboxSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        Ok(self.core.account_reads.list_smart_mailboxes()?)
    }

    async fn get_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<posthaste_domain::SmartMailbox, RuntimeError> {
        self.ensure_runtime_active()?;
        Ok(self
            .core
            .account_reads
            .get_smart_mailbox(&smart_mailbox_id)?)
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
        Ok(self.core.account_reads.list_tags(scope)?)
    }

    async fn get_identity(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<posthaste_domain::Identity, RuntimeError> {
        self.ensure_runtime_active()?;
        let gateway = self.core.live_accounts.gateway(&account_id).await?;
        Ok(self
            .core
            .service
            .fetch_identity(&account_id, gateway.as_ref())
            .await?)
    }

    async fn list_sender_addresses(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<posthaste_domain::CachedSenderAddress>, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core
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
        self.ensure_runtime_active()?;
        let gateway = self.core.live_accounts.gateway(&account_id).await?;
        Ok(self
            .core
            .service
            .fetch_reply_context(&account_id, &message_id, gateway.as_ref())
            .await?)
    }

    async fn query_mail_page(
        &self,
        _caller: RuntimeCaller,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.ensure_runtime_active()?;
        self.core.mail_queries.query_mail_page(request).await
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
        let gateway = self.core.live_accounts.gateway(&account_id).await?;
        self.core
            .service
            .send_message(&account_id, &request, gateway.as_ref())
            .await?;
        if let Some(sender) = &request.from {
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
        if let Err(error) = self
            .core
            .live_accounts
            .trigger_account_sync(&account_id, SyncTrigger::Manual)
            .await
        {
            ph_warn!(
                events::SEND_FOLLOWUP_SYNC_TRIGGER_FAILED,
                source_id = %account_id,
                error = %error,
                "send accepted but follow-up sync trigger failed"
            );
        }
        Ok(())
    }

    async fn set_message_keywords(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: SetKeywordsCommand,
    ) -> Result<posthaste_domain::CommandResult, RuntimeError> {
        self.ensure_runtime_active()?;
        let gateway = self.core.live_accounts.gateway(&account_id).await?;
        let result = self
            .core
            .service
            .set_keywords(&account_id, &message_id, &command, gateway.as_ref())
            .await?;
        self.publish_events(&result.events);
        Ok(result)
    }

    async fn add_message_to_mailbox(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: AddToMailboxCommand,
    ) -> Result<posthaste_domain::CommandResult, RuntimeError> {
        self.ensure_runtime_active()?;
        let gateway = self.core.live_accounts.gateway(&account_id).await?;
        let result = self
            .core
            .service
            .add_to_mailbox(&account_id, &message_id, &command, gateway.as_ref())
            .await?;
        self.publish_events(&result.events);
        Ok(result)
    }

    async fn remove_message_from_mailbox(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: RemoveFromMailboxCommand,
    ) -> Result<posthaste_domain::CommandResult, RuntimeError> {
        self.ensure_runtime_active()?;
        let gateway = self.core.live_accounts.gateway(&account_id).await?;
        let result = self
            .core
            .service
            .remove_from_mailbox(&account_id, &message_id, &command, gateway.as_ref())
            .await?;
        self.publish_events(&result.events);
        Ok(result)
    }

    async fn replace_message_mailboxes(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        command: ReplaceMailboxesCommand,
    ) -> Result<posthaste_domain::CommandResult, RuntimeError> {
        self.ensure_runtime_active()?;
        let gateway = self.core.live_accounts.gateway(&account_id).await?;
        let result = self
            .core
            .service
            .replace_mailboxes(&account_id, &message_id, &command, gateway.as_ref())
            .await?;
        self.publish_events(&result.events);
        Ok(result)
    }

    async fn destroy_message(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<posthaste_domain::CommandResult, RuntimeError> {
        self.ensure_runtime_active()?;
        let gateway = self.core.live_accounts.gateway(&account_id).await?;
        let result = self
            .core
            .service
            .destroy_message(&account_id, &message_id, gateway.as_ref())
            .await?;
        self.publish_events(&result.events);
        Ok(result)
    }

    async fn set_mailbox_role(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        mailbox_id: MailboxId,
        role: Option<String>,
    ) -> Result<Vec<MailboxSummary>, RuntimeError> {
        self.ensure_runtime_active()?;
        let gateway = self.core.live_accounts.gateway(&account_id).await?;
        let events = self
            .core
            .service
            .set_mailbox_role(&account_id, &mailbox_id, role.as_deref(), gateway.as_ref())
            .await?;
        self.publish_events(&events);
        Ok(self.core.service.list_mailboxes(&account_id)?)
    }

    async fn get_message_detail(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<posthaste_domain::CommandResult, RuntimeError> {
        self.ensure_runtime_active()?;
        let gateway = self.optional_gateway(&account_id).await;
        let result = self
            .core
            .service
            .get_message_detail(&account_id, &message_id, gateway.as_deref())
            .await?;
        self.publish_events(&result.events);
        Ok(result)
    }

    async fn get_message_attachment(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
        attachment_id: String,
    ) -> Result<RuntimeAttachmentBytes, RuntimeError> {
        self.ensure_runtime_active()?;
        let gateway = self.optional_gateway(&account_id).await;
        let result = self
            .core
            .service
            .get_message_detail(&account_id, &message_id, gateway.as_deref())
            .await?;
        self.publish_events(&result.events);
        let detail = result
            .detail
            .ok_or_else(|| RuntimeError::not_found("message detail not available"))?;
        let attachment = detail
            .attachments
            .into_iter()
            .find(|attachment| attachment.id == attachment_id)
            .ok_or_else(|| RuntimeError::not_found("attachment not found"))?;
        let gateway = gateway.ok_or_else(|| {
            RuntimeError::retryable(
                RuntimeErrorCode::ProviderUnavailable,
                format!("gateway unavailable for account {account_id}"),
            )
        })?;
        let bytes = self
            .core
            .service
            .download_blob(&account_id, &attachment.blob_id, gateway.as_ref())
            .await?;
        Ok(RuntimeAttachmentBytes {
            bytes,
            mime_type: attachment.mime_type,
            filename: attachment.filename,
        })
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
        Ok(self.core.service.list_events(&filter)?)
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
