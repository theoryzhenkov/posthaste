use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{future::pending, StreamExt};
use posthaste_domain::{
    AccountDriver, AccountId, AccountRuntimeOverview, AccountSettings, AccountStatus, DomainEvent,
    GatewayError, Identity, MailService, MailStore, PushEventStream, PushStatus, PushStreamEvent,
    ResilientPushConfig, SecretStore, ServiceError, SharedGateway, SyncTrigger,
    EVENT_TOPIC_ACCOUNT_STATUS_CHANGED, EVENT_TOPIC_PUSH_CONNECTED, EVENT_TOPIC_PUSH_DISCONNECTED,
};
use posthaste_engine::{connect_jmap_client, LiveJmapGateway, MockJmapGateway};
use serde_json::json;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, info_span, warn, Instrument};

use crate::push::resilient_push_stream;

const AUTOMATION_BACKFILL_BATCH_SIZE: usize = 10;
const AUTOMATION_BACKFILL_INITIAL_DELAY: Duration = Duration::from_secs(10);
const AUTOMATION_BACKFILL_INTERVAL: Duration = Duration::from_secs(15);

/// Manages per-account async runtimes: connection lifecycle, sync triggers,
/// push stream consumption, and runtime status tracking.
///
/// @spec docs/L1-sync#sync-loop
/// @spec docs/L1-api#account-crud-lifecycle
pub struct AccountSupervisor {
    shared: Arc<SupervisorShared>,
    runtimes: RwLock<HashMap<String, ManagedRuntime>>,
}

/// Shared state across all account runtimes: services, event bus, and runtime overviews.
struct SupervisorShared {
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    secret_store: Arc<dyn SecretStore>,
    event_sender: broadcast::Sender<DomainEvent>,
    gateways: RwLock<HashMap<String, SharedGateway>>,
    runtime_overviews: RwLock<HashMap<String, AccountRuntimeOverview>>,
    poll_interval: Duration,
}

/// A running account task and its command channel.
struct ManagedRuntime {
    command_tx: mpsc::Sender<RuntimeCommand>,
    handle: JoinHandle<()>,
}

/// Commands sent to a running account runtime via the mpsc channel.
enum RuntimeCommand {
    Trigger {
        trigger: SyncTrigger,
        reply: oneshot::Sender<Result<usize, ServiceError>>,
    },
}

/// Result of `POST /v1/accounts/{id}/verify` — JMAP session discovery outcome.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub struct AccountVerification {
    pub ok: bool,
    pub identity: Option<Identity>,
    pub push_supported: bool,
}

/// A live gateway connection paired with its optional push event stream.
struct AccountConnection {
    gateway: SharedGateway,
    push_events: Option<PushEventStream>,
}

impl AccountSupervisor {
    /// Create a supervisor with shared services and the configured poll interval.
    pub fn new(
        service: Arc<MailService>,
        store: Arc<dyn MailStore>,
        secret_store: Arc<dyn SecretStore>,
        event_sender: broadcast::Sender<DomainEvent>,
        poll_interval: Duration,
    ) -> Self {
        Self {
            shared: Arc::new(SupervisorShared {
                service,
                store,
                secret_store,
                event_sender,
                gateways: RwLock::new(HashMap::new()),
                runtime_overviews: RwLock::new(HashMap::new()),
                poll_interval,
            }),
            runtimes: RwLock::new(HashMap::new()),
        }
    }

    /// Start (or restart) the async runtime for an account. Stops any
    /// existing runtime first. Disabled accounts get a `Disabled` status
    /// without spawning a task.
    pub async fn start_account(&self, account: &AccountSettings) {
        self.stop_account(&account.id).await;
        if !account.enabled {
            info!(account_id = %account.id, "account disabled, skipping runtime");
            self.shared
                .set_runtime_overview(
                    &account.id,
                    AccountRuntimeOverview {
                        status: AccountStatus::Disabled,
                        push: PushStatus::Disabled,
                        ..Default::default()
                    },
                )
                .await;
            return;
        }

        info!(account_id = %account.id, driver = ?account.driver, "starting account runtime");
        let (command_tx, command_rx) = mpsc::channel(32);
        let shared = self.shared.clone();
        let account = account.clone();
        let account_id = account.id.clone();
        let span = info_span!("supervisor.runtime", account_id = %account_id);
        let handle = tokio::spawn(
            async move {
                run_account_runtime(shared, account, command_rx).await;
            }
            .instrument(span),
        );
        self.runtimes.write().await.insert(
            account_id.to_string(),
            ManagedRuntime { command_tx, handle },
        );
    }

    /// Stop the runtime task and remove the gateway for an account.
    pub async fn stop_account(&self, account_id: &AccountId) {
        if let Some(runtime) = self.runtimes.write().await.remove(account_id.as_str()) {
            info!(account_id = %account_id, "stopping account runtime");
            runtime.handle.abort();
        }
        self.shared.remove_gateway(account_id).await;
    }

    /// Stop the runtime and clear runtime overview state for a deleted account.
    pub async fn remove_account(&self, account_id: &AccountId) {
        info!(account_id = %account_id, "removing account");
        self.stop_account(account_id).await;
        self.shared
            .runtime_overviews
            .write()
            .await
            .remove(account_id.as_str());
    }

    /// Send a manual sync trigger to the account runtime and await its result.
    ///
    /// @spec docs/L1-api#sync-and-events
    pub async fn sync_account(&self, account_id: &AccountId) -> Result<usize, ServiceError> {
        let runtimes = self.runtimes.read().await;
        let runtime = runtimes
            .get(account_id.as_str())
            .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()))?;
        let (reply_tx, reply_rx) = oneshot::channel();
        runtime
            .command_tx
            .send(RuntimeCommand::Trigger {
                trigger: SyncTrigger::Manual,
                reply: reply_tx,
            })
            .await
            .map_err(|_| GatewayError::Unavailable(account_id.to_string()))?;
        reply_rx
            .await
            .map_err(|_| ServiceError::from(GatewayError::Unavailable(account_id.to_string())))?
    }

    /// Get the current runtime status snapshot for an account.
    pub async fn runtime_overview(&self, account_id: &AccountId) -> AccountRuntimeOverview {
        self.shared.runtime_overview(account_id).await
    }

    /// Return the live gateway for an account, if its runtime is connected.
    pub async fn gateway(&self, account_id: &AccountId) -> Result<SharedGateway, ServiceError> {
        self.shared.gateway(account_id).await
    }

    /// Attempt JMAP session discovery for an account without starting a
    /// persistent runtime.
    ///
    /// @spec docs/L1-api#account-crud-lifecycle
    pub async fn verify_account(
        &self,
        account: &AccountSettings,
    ) -> Result<AccountVerification, ServiceError> {
        let conn = build_connection(account, self.shared.secret_store.as_ref()).await?;
        let identity = conn.gateway.fetch_identity(&account.id).await.ok();
        Ok(AccountVerification {
            ok: true,
            identity,
            push_supported: matches!(account.driver, AccountDriver::Jmap),
        })
    }
}

/// Main event loop for an account: polls on timer, push notifications, and
/// manual sync commands. Runs until the task is aborted.
///
/// @spec docs/L1-sync#sync-loop
async fn run_account_runtime(
    shared: Arc<SupervisorShared>,
    account: AccountSettings,
    mut command_rx: mpsc::Receiver<RuntimeCommand>,
) {
    let account_id = account.id.clone();
    let mut connection: Option<AccountConnection> = None;
    let mut interval = tokio::time::interval(shared.poll_interval);
    let mut backfill_interval = tokio::time::interval_at(
        tokio::time::Instant::now() + AUTOMATION_BACKFILL_INITIAL_DELAY,
        AUTOMATION_BACKFILL_INTERVAL,
    );
    backfill_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    shared
        .set_runtime_overview(
            &account_id,
            AccountRuntimeOverview {
                status: AccountStatus::Offline,
                push: PushStatus::Reconnecting,
                ..Default::default()
            },
        )
        .await;

    // Initial sync + gateway setup
    let _ = process_sync_trigger(
        &shared,
        &account,
        SyncTrigger::Startup,
        &mut connection,
        None,
    )
    .await;

    loop {
        let next_push = async {
            match connection.as_mut().and_then(|c| c.push_events.as_mut()) {
                Some(stream) => stream.next().await,
                None => pending().await,
            }
        };

        tokio::select! {
            _ = interval.tick() => {
                let _ = process_sync_trigger(
                    &shared, &account, SyncTrigger::Poll, &mut connection, None,
                ).await;
            }
            _ = backfill_interval.tick() => {
                let _ = process_automation_backfill_batch(
                    &shared,
                    &account_id,
                    connection.as_ref().map(|connection| connection.gateway.clone()),
                ).await;
            }
            Some(command) = command_rx.recv() => {
                match command {
                    RuntimeCommand::Trigger { trigger, reply } => {
                        let _ = process_sync_trigger(
                            &shared, &account, trigger, &mut connection, Some(reply),
                        ).await;
                    }
                }
            }
            Some(event) = next_push => {
                match event {
                    PushStreamEvent::Notification(ref notification) => {
                        debug!(
                            account_id = %account_id,
                            changed = ?notification.changed,
                            "push notification received"
                        );
                        let _ = process_sync_trigger(
                            &shared, &account, SyncTrigger::Push, &mut connection, None,
                        ).await;
                    }
                    PushStreamEvent::Connected { transport } => {
                        info!(account_id = %account_id, transport, "push connected");
                        shared.set_push_status(&account_id, PushStatus::Connected).await;
                    }
                    PushStreamEvent::Disconnected { transport, reason } => {
                        warn!(account_id = %account_id, transport, reason = %reason, "push disconnected");
                        shared.handle_push_disconnect(&account_id, &format!("{transport}: {reason}")).await;
                    }
                    PushStreamEvent::Fallback { from, to } => {
                        warn!(account_id = %account_id, from, to, "push falling back");
                        shared.handle_push_disconnect(
                            &account_id,
                            &format!("falling back from {from} to {to}"),
                        ).await;
                    }
                }
            }
        }
    }
}

async fn process_automation_backfill_batch(
    shared: &Arc<SupervisorShared>,
    account_id: &AccountId,
    gateway: Option<SharedGateway>,
) -> bool {
    let Some(gateway) = gateway else {
        return true;
    };

    match shared
        .service
        .backfill_automation_rules_batch(
            account_id,
            gateway.as_ref(),
            AUTOMATION_BACKFILL_BATCH_SIZE,
        )
        .await
    {
        Ok((events, has_more)) => {
            if !events.is_empty() {
                info!(
                    account_id = %account_id,
                    event_count = events.len(),
                    has_more,
                    "automation backfill batch completed"
                );
                shared.publish_events(&events);
            }
            has_more
        }
        Err(error) => {
            warn!(
                account_id = %account_id,
                error = %error,
                "automation backfill batch failed"
            );
            true
        }
    }
}

/// Execute one sync cycle: ensure connection, run sync, publish events,
/// and update runtime status. On failure, tears down the connection and
/// records the error.
///
/// @spec docs/L1-sync#sync-loop
/// @spec docs/L1-sync#error-handling
async fn process_sync_trigger(
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    trigger: SyncTrigger,
    connection: &mut Option<AccountConnection>,
    reply: Option<oneshot::Sender<Result<usize, ServiceError>>>,
) -> Result<(), ServiceError> {
    let account_id = account.id.clone();
    debug!(account_id = %account_id, trigger = ?trigger, "sync triggered");
    shared
        .set_status_only(&account_id, AccountStatus::Syncing)
        .await;

    let result = match ensure_connection(shared, account, connection).await {
        Ok(()) => {
            if let Some(connection) = connection.as_ref() {
                shared
                    .service
                    .sync_account(&account_id, trigger.clone(), connection.gateway.as_ref())
                    .await
            } else {
                Err(GatewayError::Unavailable(account_id.to_string()).into())
            }
        }
        Err(error) => Err(error),
    };

    match result {
        Ok(events) => {
            let event_count = events.len();
            info!(account_id = %account_id, event_count, "sync completed");
            shared.publish_events(&events);
            shared.mark_sync_success(&account_id).await;
            if let Some(reply) = reply {
                let _ = reply.send(Ok(event_count));
            }
        }
        Err(error) => {
            shared.remove_gateway(&account_id).await;
            *connection = None; // tears down gateway + push stream together
            let stage = if matches!(
                error,
                ServiceError::Gateway(GatewayError::Unavailable(_))
                    | ServiceError::Gateway(GatewayError::Auth)
                    | ServiceError::Gateway(GatewayError::Network(_))
            ) {
                "connect"
            } else {
                "sync"
            };
            error!(account_id = %account_id, error = %error, stage, "sync failed");
            if let Ok(event) = shared.service.record_sync_failure(
                &account_id,
                error.code(),
                &error.to_string(),
                trigger,
                stage,
            ) {
                shared.publish_events(&[event]);
            }
            shared.mark_sync_failure(&account_id, &error).await;
            if let Some(reply) = reply {
                let _ = reply.send(Err(error));
            }
        }
    }

    Ok(())
}

/// Lazily establish the gateway connection and push stream if not already
/// connected.
async fn ensure_connection(
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    connection: &mut Option<AccountConnection>,
) -> Result<(), ServiceError> {
    if connection.is_some() {
        return Ok(());
    }
    debug!(account_id = %account.id, "establishing connection");
    let conn = build_connection(account, shared.secret_store.as_ref()).await?;
    shared.set_gateway(&account.id, conn.gateway.clone()).await;
    *connection = Some(conn);
    info!(account_id = %account.id, "connection established");
    Ok(())
}

/// Build a gateway connection for an account, resolving its secret and
/// opening a resilient push stream (WS preferred, SSE fallback).
///
/// @spec docs/L2-transport#transport-negotiation
async fn build_connection(
    account: &AccountSettings,
    secret_store: &dyn SecretStore,
) -> Result<AccountConnection, ServiceError> {
    match account.driver {
        AccountDriver::Mock => Ok(AccountConnection {
            gateway: Arc::new(MockJmapGateway::default()),
            push_events: None,
        }),
        AccountDriver::Jmap => {
            let url = account
                .transport
                .base_url
                .as_deref()
                .ok_or_else(|| GatewayError::Rejected("missing JMAP base URL".to_string()))?;
            let username = account
                .transport
                .username
                .as_deref()
                .map(str::trim)
                .filter(|username| !username.is_empty());
            let secret_ref = account.transport.secret_ref.as_ref().ok_or_else(|| {
                GatewayError::Rejected("missing JMAP secret reference".to_string())
            })?;
            let secret = secret_store.resolve(secret_ref)?;
            let client = connect_jmap_client(url, username, &secret).await?;
            let gateway: SharedGateway = Arc::new(LiveJmapGateway::from_client(client));

            let transports = gateway.push_transports();
            let mut transports = transports.into_iter();
            let primary = transports.next();
            let fallback = transports.next();

            info!(
                account_id = %account.id,
                primary = primary.as_ref().map(|t| t.name()),
                fallback = fallback.as_ref().map(|t| t.name()),
                reason = if primary.as_ref().map(|t| t.name()) == Some("ws") {
                    "server advertises WebSocket push support"
                } else {
                    "WebSocket not available, SSE only"
                },
                "push transport negotiation complete"
            );

            let push_events = primary.map(|primary| {
                resilient_push_stream(
                    account.id.clone(),
                    primary,
                    fallback,
                    ResilientPushConfig::default(),
                )
            });

            Ok(AccountConnection {
                gateway,
                push_events,
            })
        }
    }
}

impl SupervisorShared {
    async fn gateway(&self, account_id: &AccountId) -> Result<SharedGateway, ServiceError> {
        self.gateways
            .read()
            .await
            .get(account_id.as_str())
            .cloned()
            .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()).into())
    }

    async fn set_gateway(&self, account_id: &AccountId, gateway: SharedGateway) {
        self.gateways
            .write()
            .await
            .insert(account_id.to_string(), gateway);
    }

    async fn remove_gateway(&self, account_id: &AccountId) {
        self.gateways.write().await.remove(account_id.as_str());
    }

    /// Broadcast domain events to all SSE subscribers.
    fn publish_events(&self, events: &[DomainEvent]) {
        for event in events {
            let _ = self.event_sender.send(event.clone());
        }
    }

    /// Read the cached runtime overview for an account, defaulting to empty.
    async fn runtime_overview(&self, account_id: &AccountId) -> AccountRuntimeOverview {
        self.runtime_overviews
            .read()
            .await
            .get(account_id.as_str())
            .cloned()
            .unwrap_or_default()
    }

    /// Update only the account status, preserving other overview fields.
    async fn set_status_only(&self, account_id: &AccountId, status: AccountStatus) {
        let mut current = self.runtime_overview(account_id).await;
        current.status = status;
        self.set_runtime_overview(account_id, current).await;
    }

    /// Update only the push status, preserving other overview fields.
    async fn set_push_status(&self, account_id: &AccountId, push: PushStatus) {
        let mut current = self.runtime_overview(account_id).await;
        current.push = push;
        self.set_runtime_overview(account_id, current).await;
    }

    /// Record a successful sync: set status to Ready, clear error, update timestamp.
    async fn mark_sync_success(&self, account_id: &AccountId) {
        let mut current = self.runtime_overview(account_id).await;
        current.status = AccountStatus::Ready;
        current.last_sync_at = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .ok();
        current.last_sync_error = None;
        current.last_sync_error_code = None;
        if matches!(current.push, PushStatus::Disabled) {
            current.push = PushStatus::Reconnecting;
        }
        self.set_runtime_overview(account_id, current).await;
    }

    /// Record a sync failure: derive status from error type, store error details.
    async fn mark_sync_failure(&self, account_id: &AccountId, error: &ServiceError) {
        let mut current = self.runtime_overview(account_id).await;
        current.status = match error {
            ServiceError::Gateway(GatewayError::Auth) => AccountStatus::AuthError,
            ServiceError::Gateway(GatewayError::Network(_))
            | ServiceError::Gateway(GatewayError::Unavailable(_))
            | ServiceError::Secret(_) => AccountStatus::Offline,
            _ => AccountStatus::Degraded,
        };
        current.last_sync_error = Some(error.to_string());
        current.last_sync_error_code = Some(error.code().to_string());
        if !matches!(current.push, PushStatus::Unsupported | PushStatus::Disabled) {
            current.push = PushStatus::Reconnecting;
        }
        self.set_runtime_overview(account_id, current).await;
    }

    /// Handle a push stream disconnect: emit event and set push status to Reconnecting.
    async fn handle_push_disconnect(&self, account_id: &AccountId, message: &str) {
        let event = self
            .store
            .append_event(
                account_id,
                EVENT_TOPIC_PUSH_DISCONNECTED,
                None,
                None,
                json!({ "message": message }),
            )
            .ok();
        if let Some(event) = event {
            self.publish_events(&[event]);
        }
        self.set_push_status(account_id, PushStatus::Reconnecting)
            .await;
    }

    /// Persist a runtime overview and emit status/push change events when transitions occur.
    ///
    /// @spec docs/L1-sync#event-propagation
    async fn set_runtime_overview(&self, account_id: &AccountId, overview: AccountRuntimeOverview) {
        let previous = self
            .runtime_overviews
            .write()
            .await
            .insert(account_id.to_string(), overview.clone());

        let mut side_effects = Vec::new();
        if previous.as_ref().map(|item| &item.status) != Some(&overview.status) {
            if let Ok(event) = self.store.append_event(
                account_id,
                EVENT_TOPIC_ACCOUNT_STATUS_CHANGED,
                None,
                None,
                json!({
                    "status": format!("{:?}", overview.status).to_lowercase(),
                    "push": format!("{:?}", overview.push).to_lowercase(),
                    "lastSyncAt": overview.last_sync_at,
                    "lastSyncError": overview.last_sync_error,
                    "lastSyncErrorCode": overview.last_sync_error_code,
                }),
            ) {
                side_effects.push(event);
            }
        }

        match (previous.as_ref().map(|item| &item.push), &overview.push) {
            (Some(PushStatus::Connected), PushStatus::Connected) => {}
            (_, PushStatus::Connected) => {
                if let Ok(event) = self.store.append_event(
                    account_id,
                    EVENT_TOPIC_PUSH_CONNECTED,
                    None,
                    None,
                    json!({}),
                ) {
                    side_effects.push(event);
                }
            }
            (Some(PushStatus::Connected), _) => {
                if let Ok(event) = self.store.append_event(
                    account_id,
                    EVENT_TOPIC_PUSH_DISCONNECTED,
                    None,
                    None,
                    json!({}),
                ) {
                    side_effects.push(event);
                }
            }
            _ => {}
        }

        self.publish_events(&side_effects);
    }
}
