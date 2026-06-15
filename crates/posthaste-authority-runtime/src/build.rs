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
use posthaste_runtime_contract::{
    AccountScopeRequest, AccountVerificationResult, CreateAccountMutation, PatchAccountMutation,
    RuntimeAccountList, RuntimeAdapterError, RuntimeAttachmentBytes, RuntimeCaller, RuntimeCore,
    RuntimeError, RuntimeErrorCode, RuntimeEventSubscription, RuntimeLifecycle, RuntimeStatus,
    RuntimeStoreStatus,
};
use posthaste_store::DatabaseStore;
use thiserror::Error;
use tokio::sync::broadcast;

use crate::account_mutations::AccountMutationService;
use crate::account_reads::{AccountReadService, DefaultAccountRuntimeOverviewProvider};
use crate::bootstrap::initialize_config;
use crate::oauth::{OAuthExchangeResult, OAuthProviderProfile, OAuthTokenSet};
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
    let account_mutations = Arc::new(AccountMutationService::new(
        api_bridge.service.clone(),
        api_bridge.store.clone(),
        api_bridge.secret_store.clone(),
        api_bridge.event_sender.clone(),
        account_supervisor.clone(),
        account_reads.clone(),
    ));
    let core = Arc::new(AuthorityRuntimeCore {
        api_bridge: api_bridge.clone(),
        account_reads,
        account_mutations: Some(account_mutations),
        live_accounts: account_supervisor.clone(),
        startup_status: runtime_status.clone(),
        stopped: stopped.clone(),
    });

    Ok(AuthorityRuntimeBuild {
        handle: AuthorityRuntimeHandle { core },
        shutdown: RuntimeShutdownHandle { stopped },
        runtime_status,
        account_supervisor,
        api_bridge,
    })
}

struct AuthorityRuntimeCore {
    #[allow(dead_code)]
    api_bridge: AuthorityRuntimeApiMigrationBridge,
    account_reads: Arc<AccountReadService>,
    account_mutations: Option<Arc<AccountMutationService>>,
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
            ServiceErrorKind::Conflict => RuntimeErrorCode::Conflict,
            ServiceErrorKind::StateMismatch => RuntimeErrorCode::StateMismatch,
            ServiceErrorKind::AuthError => RuntimeErrorCode::Unauthorized,
            ServiceErrorKind::GatewayUnavailable => RuntimeErrorCode::ProviderUnavailable,
            ServiceErrorKind::NetworkError => RuntimeErrorCode::NetworkError,
            ServiceErrorKind::CannotCalculateChanges => RuntimeErrorCode::CannotCalculateChanges,
            ServiceErrorKind::GatewayRejected => RuntimeErrorCode::GatewayRejected,
            ServiceErrorKind::SecretUnavailable => RuntimeErrorCode::SecretUnavailable,
            ServiceErrorKind::SecretUnsupported => RuntimeErrorCode::SecretUnsupported,
            ServiceErrorKind::StorageFailure => RuntimeErrorCode::StorageFailure,
            ServiceErrorKind::ConfigValidation => RuntimeErrorCode::ConfigValidation,
            ServiceErrorKind::ConfigIo => RuntimeErrorCode::ConfigIo,
            ServiceErrorKind::ConfigParse => RuntimeErrorCode::ConfigParse,
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
        let account_mutations = account_supervisor.map(|supervisor| {
            Arc::new(AccountMutationService::new(
                api_bridge.service.clone(),
                api_bridge.store.clone(),
                api_bridge.secret_store.clone(),
                api_bridge.event_sender.clone(),
                supervisor,
                account_reads.clone(),
            ))
        });
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
                account_mutations,
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

    fn account_mutations(&self) -> Result<Arc<AccountMutationService>, RuntimeError> {
        self.core.account_mutations.clone().ok_or_else(|| {
            RuntimeError(RuntimeAdapterError {
                code: RuntimeErrorCode::RuntimeNotReady,
                message: "account mutation runtime is not available".to_string(),
                retryable: false,
                correlation_id: None,
                details: serde_json::Value::Null,
            })
        })
    }

    pub async fn create_oauth_account_from_exchange(
        &self,
        profile: &OAuthProviderProfile,
        exchange: OAuthExchangeResult,
    ) -> Result<posthaste_domain::AccountOverview, RuntimeError> {
        self.account_mutations()?
            .create_oauth_account_from_exchange(profile, exchange)
            .await
    }

    pub async fn persist_oauth_token_set(
        &self,
        account_id: AccountId,
        token_set: OAuthTokenSet,
    ) -> Result<posthaste_domain::AccountOverview, RuntimeError> {
        self.account_mutations()?
            .persist_oauth_token_set(account_id, token_set)
            .await
    }

    fn publish_events(&self, events: &[DomainEvent]) {
        for event in events {
            let _ = self.core.api_bridge.event_sender.send(event.clone());
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

    async fn patch_app_settings(
        &self,
        _caller: RuntimeCaller,
        mutation: posthaste_runtime_contract::PatchAppSettingsMutation,
    ) -> Result<AppSettings, RuntimeError> {
        self.account_mutations()?.patch_app_settings(mutation)
    }

    async fn preview_automation_rule(
        &self,
        _caller: RuntimeCaller,
        mutation: posthaste_runtime_contract::AutomationRulePreviewMutation,
    ) -> Result<posthaste_runtime_contract::AutomationRulePreviewResult, RuntimeError> {
        self.account_mutations()?.preview_automation_rule(mutation)
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

    async fn create_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        mutation: posthaste_runtime_contract::CreateSmartMailboxMutation,
    ) -> Result<posthaste_domain::SmartMailbox, RuntimeError> {
        self.account_mutations()?.create_smart_mailbox(mutation)
    }

    async fn patch_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
        mutation: posthaste_runtime_contract::PatchSmartMailboxMutation,
    ) -> Result<posthaste_domain::SmartMailbox, RuntimeError> {
        self.account_mutations()?
            .patch_smart_mailbox(smart_mailbox_id, mutation)
    }

    async fn delete_smart_mailbox(
        &self,
        _caller: RuntimeCaller,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<(), RuntimeError> {
        self.account_mutations()?
            .delete_smart_mailbox(smart_mailbox_id)
    }

    async fn reset_default_smart_mailboxes(
        &self,
        _caller: RuntimeCaller,
    ) -> Result<Vec<posthaste_domain::SmartMailboxSummary>, RuntimeError> {
        self.account_mutations()?.reset_default_smart_mailboxes()
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

    async fn send_message(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        request: SendMessageRequest,
    ) -> Result<(), RuntimeError> {
        let gateway = self
            .core
            .live_accounts
            .gateway(&account_id)
            .await
            .map_err(Self::service_error_to_runtime_error)?;
        self.core
            .api_bridge
            .service
            .send_message(&account_id, &request, gateway.as_ref())
            .await
            .map_err(Self::service_error_to_runtime_error)?;
        if let Some(sender) = &request.from {
            if let Err(error) = self
                .core
                .api_bridge
                .store
                .remember_sender_address(&account_id, sender)
            {
                tracing::warn!(
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
            tracing::warn!(
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
        let gateway = self
            .core
            .live_accounts
            .gateway(&account_id)
            .await
            .map_err(Self::service_error_to_runtime_error)?;
        let result = self
            .core
            .api_bridge
            .service
            .set_keywords(&account_id, &message_id, &command, gateway.as_ref())
            .await
            .map_err(Self::service_error_to_runtime_error)?;
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
        let gateway = self
            .core
            .live_accounts
            .gateway(&account_id)
            .await
            .map_err(Self::service_error_to_runtime_error)?;
        let result = self
            .core
            .api_bridge
            .service
            .add_to_mailbox(&account_id, &message_id, &command, gateway.as_ref())
            .await
            .map_err(Self::service_error_to_runtime_error)?;
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
        let gateway = self
            .core
            .live_accounts
            .gateway(&account_id)
            .await
            .map_err(Self::service_error_to_runtime_error)?;
        let result = self
            .core
            .api_bridge
            .service
            .remove_from_mailbox(&account_id, &message_id, &command, gateway.as_ref())
            .await
            .map_err(Self::service_error_to_runtime_error)?;
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
        let gateway = self
            .core
            .live_accounts
            .gateway(&account_id)
            .await
            .map_err(Self::service_error_to_runtime_error)?;
        let result = self
            .core
            .api_bridge
            .service
            .replace_mailboxes(&account_id, &message_id, &command, gateway.as_ref())
            .await
            .map_err(Self::service_error_to_runtime_error)?;
        self.publish_events(&result.events);
        Ok(result)
    }

    async fn destroy_message(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<posthaste_domain::CommandResult, RuntimeError> {
        let gateway = self
            .core
            .live_accounts
            .gateway(&account_id)
            .await
            .map_err(Self::service_error_to_runtime_error)?;
        let result = self
            .core
            .api_bridge
            .service
            .destroy_message(&account_id, &message_id, gateway.as_ref())
            .await
            .map_err(Self::service_error_to_runtime_error)?;
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
        let gateway = self
            .core
            .live_accounts
            .gateway(&account_id)
            .await
            .map_err(Self::service_error_to_runtime_error)?;
        let events = self
            .core
            .api_bridge
            .service
            .set_mailbox_role(&account_id, &mailbox_id, role.as_deref(), gateway.as_ref())
            .await
            .map_err(Self::service_error_to_runtime_error)?;
        self.publish_events(&events);
        self.core
            .api_bridge
            .service
            .list_mailboxes(&account_id)
            .map_err(Self::service_error_to_runtime_error)
    }

    async fn get_message_detail(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<posthaste_domain::CommandResult, RuntimeError> {
        let gateway = self.optional_gateway(&account_id).await;
        let result = self
            .core
            .api_bridge
            .service
            .get_message_detail(&account_id, &message_id, gateway.as_deref())
            .await
            .map_err(Self::service_error_to_runtime_error)?;
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
        let gateway = self.optional_gateway(&account_id).await;
        let result = self
            .core
            .api_bridge
            .service
            .get_message_detail(&account_id, &message_id, gateway.as_deref())
            .await
            .map_err(Self::service_error_to_runtime_error)?;
        self.publish_events(&result.events);
        let detail = result.detail.ok_or_else(|| {
            RuntimeError(RuntimeAdapterError {
                code: RuntimeErrorCode::NotFound,
                message: "message detail not available".to_string(),
                retryable: false,
                correlation_id: None,
                details: serde_json::Value::Null,
            })
        })?;
        let attachment = detail
            .attachments
            .into_iter()
            .find(|attachment| attachment.id == attachment_id)
            .ok_or_else(|| {
                RuntimeError(RuntimeAdapterError {
                    code: RuntimeErrorCode::NotFound,
                    message: "attachment not found".to_string(),
                    retryable: false,
                    correlation_id: None,
                    details: serde_json::Value::Null,
                })
            })?;
        let gateway = gateway.ok_or_else(|| {
            RuntimeError(RuntimeAdapterError {
                code: RuntimeErrorCode::ProviderUnavailable,
                message: format!("gateway unavailable for account {account_id}"),
                retryable: true,
                correlation_id: None,
                details: serde_json::Value::Null,
            })
        })?;
        let bytes = self
            .core
            .api_bridge
            .service
            .download_blob(&account_id, &attachment.blob_id, gateway.as_ref())
            .await
            .map_err(Self::service_error_to_runtime_error)?;
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
        self.core
            .live_accounts
            .sync_account_with_mode(&account_id, mode)
            .await
            .map_err(Self::service_error_to_runtime_error)
    }

    async fn replay_events(
        &self,
        _caller: RuntimeCaller,
        filter: EventFilter,
    ) -> Result<Vec<DomainEvent>, RuntimeError> {
        self.core
            .api_bridge
            .service
            .list_events(&filter)
            .map_err(Self::service_error_to_runtime_error)
    }

    async fn subscribe_events(
        &self,
        _caller: RuntimeCaller,
        filter: EventFilter,
    ) -> Result<RuntimeEventSubscription, RuntimeError> {
        let receiver = self.core.api_bridge.event_sender.subscribe();
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
        self.account_mutations()?.create_account(mutation).await
    }

    async fn patch_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        mutation: PatchAccountMutation,
    ) -> Result<posthaste_domain::AccountOverview, RuntimeError> {
        self.account_mutations()?
            .patch_account(account_id, mutation)
            .await
    }

    async fn delete_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<(), RuntimeError> {
        self.account_mutations()?.delete_account(account_id).await
    }

    async fn verify_account(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
    ) -> Result<AccountVerificationResult, RuntimeError> {
        self.account_mutations()?.verify_account(account_id).await
    }

    async fn set_account_enabled(
        &self,
        _caller: RuntimeCaller,
        account_id: AccountId,
        enabled: bool,
    ) -> Result<(), RuntimeError> {
        self.account_mutations()?
            .set_account_enabled(account_id, enabled)
            .await
    }

    async fn reload_config(&self, _caller: RuntimeCaller) -> Result<(), RuntimeError> {
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
