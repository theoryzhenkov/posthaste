use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use futures_util::{future::pending, StreamExt};
use posthaste_domain::{
    AccountDriver, AccountId, AccountRuntimeOverview, AccountSettings, AccountStatus, DomainEvent,
    GatewayError, Identity, MailService, MailStore, ProviderAuthKind, PushEventStream, PushStatus,
    PushStreamEvent, ResilientPushConfig, SecretStore, ServiceError, SharedGateway, SyncProgress,
    SyncProgressReporter, SyncProgressStage, SyncTrigger, EVENT_TOPIC_ACCOUNT_STATUS_CHANGED,
    EVENT_TOPIC_PUSH_CONNECTED, EVENT_TOPIC_PUSH_DISCONNECTED,
};
use posthaste_engine::{connect_jmap_client, LiveJmapGateway, MockJmapGateway};
use posthaste_imap::{
    imap_idle_event_stream, ImapAdapterError, ImapConnectionConfig, LiveImapSmtpGateway,
    SmtpConnectionConfig,
};
use serde_json::json;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, info_span, warn, Instrument};
use uuid::Uuid;

use crate::oauth::{OAuthTokenService, OAuthTokenSet};
use crate::push::resilient_push_stream;

const AUTOMATION_BACKFILL_BATCH_SIZE: usize = 10;
const AUTOMATION_BACKFILL_INITIAL_DELAY: Duration = Duration::from_secs(10);
const AUTOMATION_BACKFILL_INTERVAL: Duration = Duration::from_secs(15);
const CACHE_WORKER_BATCH_SIZE: usize = 3;
const CACHE_RESCORE_BATCH_SIZE: usize = 100;
const CACHE_STALE_RESCORE_BATCH_SIZE: usize = 100;
const CACHE_BACKGROUND_PRESSURE: f64 = 0.0;
const CACHE_INTERACTIVE_PRESSURE: f64 = 1.0;
const CACHE_STALE_RESCORE_AFTER: Duration = Duration::from_secs(6 * 60 * 60);
const CACHE_WORKER_INITIAL_DELAY: Duration = Duration::from_secs(5);
const CACHE_WORKER_INTERVAL: Duration = Duration::from_secs(10);

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
    TriggerOnly {
        trigger: SyncTrigger,
    },
    CacheMaintenance {
        interactive_pressure: f64,
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

    /// Request a runtime sync without waiting for completion.
    pub async fn trigger_account_sync(
        &self,
        account_id: &AccountId,
        trigger: SyncTrigger,
    ) -> Result<(), ServiceError> {
        let runtimes = self.runtimes.read().await;
        let runtime = runtimes
            .get(account_id.as_str())
            .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()))?;
        runtime
            .command_tx
            .send(RuntimeCommand::TriggerOnly { trigger })
            .await
            .map_err(|_| GatewayError::Unavailable(account_id.to_string()))?;
        Ok(())
    }

    /// Request cache re-score/fetch work without waiting for completion.
    pub async fn trigger_cache_maintenance(
        &self,
        account_id: &AccountId,
    ) -> Result<(), ServiceError> {
        let runtimes = self.runtimes.read().await;
        let runtime = runtimes
            .get(account_id.as_str())
            .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()))?;
        runtime
            .command_tx
            .send(RuntimeCommand::CacheMaintenance {
                interactive_pressure: CACHE_INTERACTIVE_PRESSURE,
            })
            .await
            .map_err(|_| GatewayError::Unavailable(account_id.to_string()))?;
        Ok(())
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
        let conn = build_connection(account, &self.shared).await?;
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
    let mut backfill_interval = tokio::time::interval_at(
        tokio::time::Instant::now() + AUTOMATION_BACKFILL_INITIAL_DELAY,
        AUTOMATION_BACKFILL_INTERVAL,
    );
    backfill_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut cache_interval = tokio::time::interval_at(
        tokio::time::Instant::now() + CACHE_WORKER_INITIAL_DELAY,
        CACHE_WORKER_INTERVAL,
    );
    cache_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
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
    let mut interval = sync_poll_interval(shared.poll_interval);

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
                interval = sync_poll_interval(shared.poll_interval);
            }
            _ = backfill_interval.tick() => {
                let _ = process_automation_backfill_batch(
                    &shared,
                    &account_id,
                    connection.as_ref().map(|connection| connection.gateway.clone()),
                ).await;
            }
            _ = cache_interval.tick() => {
                process_cache_maintenance_batch(
                    &shared,
                    &account_id,
                    connection.as_ref().map(|connection| connection.gateway.clone()),
                    CACHE_BACKGROUND_PRESSURE,
                ).await;
            }
            Some(command) = command_rx.recv() => {
                match command {
                    RuntimeCommand::Trigger { trigger, reply } => {
                        let _ = process_sync_trigger(
                            &shared, &account, trigger, &mut connection, Some(reply),
                        ).await;
                        interval = sync_poll_interval(shared.poll_interval);
                    }
                    RuntimeCommand::TriggerOnly { trigger } => {
                        let _ = process_sync_trigger(
                            &shared, &account, trigger, &mut connection, None,
                        ).await;
                        interval = sync_poll_interval(shared.poll_interval);
                    }
                    RuntimeCommand::CacheMaintenance { interactive_pressure } => {
                        process_cache_maintenance_batch(
                            &shared,
                            &account_id,
                            connection.as_ref().map(|connection| connection.gateway.clone()),
                            interactive_pressure,
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
                        interval = sync_poll_interval(shared.poll_interval);
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

async fn process_cache_maintenance_batch(
    shared: &Arc<SupervisorShared>,
    account_id: &AccountId,
    gateway: Option<SharedGateway>,
    interactive_pressure: f64,
) {
    match shared.service.queue_stale_cache_rescore_batch(
        account_id,
        CACHE_STALE_RESCORE_AFTER,
        CACHE_STALE_RESCORE_BATCH_SIZE,
    ) {
        Ok(queued) => {
            if queued > 0 {
                debug!(
                    account_id = %account_id,
                    queued,
                    stale_after_seconds = CACHE_STALE_RESCORE_AFTER.as_secs(),
                    batch_size = CACHE_STALE_RESCORE_BATCH_SIZE,
                    "stale cache rescore candidates queued"
                );
            }
        }
        Err(error) => {
            warn!(
                account_id = %account_id,
                error = %error,
                "stale cache rescore queueing failed"
            );
        }
    }

    match shared
        .service
        .process_cache_rescore_batch(account_id, CACHE_RESCORE_BATCH_SIZE)
    {
        Ok(outcome) => {
            if outcome.updated > 0 {
                debug!(
                    account_id = %account_id,
                    scanned = outcome.scanned,
                    updated = outcome.updated,
                    skipped = outcome.skipped,
                    "cache rescore batch completed"
                );
            }
        }
        Err(error) => {
            warn!(
                account_id = %account_id,
                error = %error,
                "cache rescore batch failed"
            );
        }
    }

    let Some(gateway) = gateway else {
        debug!(
            account_id = %account_id,
            "cache worker skipped because no gateway is connected"
        );
        return;
    };

    match shared
        .service
        .process_body_cache_batch(
            account_id,
            gateway.as_ref(),
            CACHE_WORKER_BATCH_SIZE,
            interactive_pressure,
        )
        .await
    {
        Ok(outcome) => {
            if !outcome.events.is_empty() {
                shared.publish_events(&outcome.events);
            }
            if outcome.attempted > 0 || outcome.cached > 0 || outcome.failed > 0 {
                info!(
                    account_id = %account_id,
                    scanned = outcome.scanned,
                    attempted = outcome.attempted,
                    cached = outcome.cached,
                    failed = outcome.failed,
                    skipped = outcome.skipped,
                    event_count = outcome.events.len(),
                    "cache worker batch completed"
                );
            } else if outcome.skipped > 0 {
                debug!(
                    account_id = %account_id,
                    scanned = outcome.scanned,
                    skipped = outcome.skipped,
                    "cache worker skipped candidates outside current budget"
                );
            } else {
                debug!(
                    account_id = %account_id,
                    scanned = outcome.scanned,
                    "cache worker batch completed without fetch work"
                );
            }
        }
        Err(error) => {
            warn!(
                account_id = %account_id,
                error = %error,
                "cache worker batch failed"
            );
        }
    }
}

fn sync_poll_interval(poll_interval: Duration) -> tokio::time::Interval {
    let mut interval =
        tokio::time::interval_at(tokio::time::Instant::now() + poll_interval, poll_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval
}

fn sync_progress_reporter(
    shared: &Arc<SupervisorShared>,
    account_id: AccountId,
    sync_id: String,
    trigger: SyncTrigger,
    started_at: String,
) -> SyncProgressReporter {
    let shared = shared.clone();
    SyncProgressReporter::new(sync_id, trigger, started_at, move |progress| {
        let shared = shared.clone();
        let account_id = account_id.clone();
        tokio::spawn(async move {
            shared.set_sync_progress(&account_id, Some(progress)).await;
        });
    })
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
        .process_automation_backfill_job_batch(
            account_id,
            gateway.as_ref(),
            AUTOMATION_BACKFILL_BATCH_SIZE,
        )
        .await
    {
        Ok(outcome) => {
            if !outcome.ran {
                return false;
            }
            let events = outcome.events;
            let has_more = outcome.has_more;
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
    let sync_id = Uuid::new_v4().to_string();
    let span = info_span!(
        "sync.cycle",
        account_id = %account_id,
        sync_id = %sync_id,
        trigger = trigger.as_str()
    );

    process_sync_trigger_inner(shared, account, trigger, connection, reply, sync_id)
        .instrument(span)
        .await
}

async fn process_sync_trigger_inner(
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    trigger: SyncTrigger,
    connection: &mut Option<AccountConnection>,
    reply: Option<oneshot::Sender<Result<usize, ServiceError>>>,
    sync_id: String,
) -> Result<(), ServiceError> {
    let account_id = account.id.clone();
    let started = Instant::now();
    let started_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| GatewayError::Rejected(error.to_string()))?;
    info!(
        account_id = %account_id,
        sync_id = %sync_id,
        trigger = trigger.as_str(),
        "sync started"
    );
    shared
        .set_sync_progress(
            &account_id,
            Some(SyncProgress {
                sync_id: sync_id.clone(),
                trigger: trigger.clone(),
                started_at: started_at.clone(),
                stage: SyncProgressStage::Connecting,
                detail: "Connecting account".to_string(),
                mailbox_name: None,
                mailbox_index: None,
                mailbox_count: None,
                message_count: None,
                total_count: None,
            }),
        )
        .await;

    let result = match ensure_connection(shared, account, connection).await {
        Ok(()) => {
            if let Some(connection) = connection.as_ref() {
                let progress = sync_progress_reporter(
                    shared,
                    account_id.clone(),
                    sync_id.clone(),
                    trigger.clone(),
                    started_at.clone(),
                );
                shared
                    .service
                    .sync_account(
                        &account_id,
                        trigger.clone(),
                        connection.gateway.as_ref(),
                        Some(progress),
                    )
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
            info!(
                account_id = %account_id,
                sync_id = %sync_id,
                trigger = trigger.as_str(),
                event_count,
                duration_ms = started.elapsed().as_millis() as u64,
                "sync completed"
            );
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
            error!(
                account_id = %account_id,
                sync_id = %sync_id,
                trigger = trigger.as_str(),
                error = %error,
                stage,
                duration_ms = started.elapsed().as_millis() as u64,
                "sync failed"
            );
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
    let conn = build_connection(account, shared).await?;
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
    shared: &Arc<SupervisorShared>,
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
            info!(
                account_id = %account.id,
                driver = "jmap",
                target_url = url,
                has_username = username.is_some(),
                "connecting account gateway"
            );
            let secret = resolve_account_secret(account, shared, secret_ref).await?;
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
        AccountDriver::ImapSmtp => {
            let secret_ref = account.transport.secret_ref.as_ref().ok_or_else(|| {
                GatewayError::Rejected("missing IMAP/SMTP secret reference".to_string())
            })?;
            let secret = resolve_account_secret(account, shared, secret_ref).await?;
            let imap_config =
                ImapConnectionConfig::from_account_transport(&account.transport, secret.clone())
                    .map_err(imap_adapter_error)?;
            let smtp_config = SmtpConnectionConfig::from_account_settings(account, secret)
                .map_err(imap_adapter_error)?;
            info!(
                account_id = %account.id,
                driver = "imap_smtp",
                imap_host = %imap_config.host,
                imap_port = imap_config.port,
                imap_security = ?imap_config.security,
                smtp_host = %smtp_config.host,
                smtp_port = smtp_config.port,
                smtp_security = ?smtp_config.security,
                auth = ?imap_config.auth,
                "connecting account gateway"
            );
            let gateway = LiveImapSmtpGateway::connect(
                imap_config.clone(),
                smtp_config,
                Some(shared.store.clone()),
            )
            .await
            .map_err(imap_adapter_error)?;
            let idle_mailbox_name = gateway
                .discovery()
                .mailboxes
                .iter()
                .find(|mailbox| mailbox.selectable && mailbox.role == Some("inbox"))
                .or_else(|| {
                    gateway
                        .discovery()
                        .mailboxes
                        .iter()
                        .find(|mailbox| mailbox.selectable)
                })
                .map(|mailbox| mailbox.name.clone());
            info!(
                account_id = %account.id,
                mailbox_count = gateway.discovery().mailboxes.len(),
                "IMAP discovery complete"
            );
            let push_events = if gateway.discovery().capabilities.supports_idle() {
                if let Some(mailbox_name) = idle_mailbox_name {
                    info!(
                        account_id = %account.id,
                        mailbox_name,
                        "IMAP IDLE push hint enabled"
                    );
                    Some(imap_idle_event_stream(
                        account.id.clone(),
                        imap_config,
                        mailbox_name,
                    ))
                } else {
                    warn!(
                        account_id = %account.id,
                        "IMAP IDLE advertised but no selectable mailbox is available"
                    );
                    shared
                        .set_push_status(&account.id, PushStatus::Unsupported)
                        .await;
                    None
                }
            } else {
                info!(
                    account_id = %account.id,
                    "IMAP IDLE unavailable; using periodic poll only"
                );
                shared
                    .set_push_status(&account.id, PushStatus::Unsupported)
                    .await;
                None
            };
            Ok(AccountConnection {
                gateway: Arc::new(gateway),
                push_events,
            })
        }
    }
}

async fn resolve_account_secret(
    account: &AccountSettings,
    shared: &Arc<SupervisorShared>,
    secret_ref: &posthaste_domain::SecretRef,
) -> Result<String, ServiceError> {
    let secret = shared.secret_store.resolve(secret_ref)?;
    if account.transport.auth != ProviderAuthKind::OAuth2 {
        return Ok(secret);
    }

    let token_set = OAuthTokenSet::decode(&secret)?;
    let token_service = OAuthTokenService::new()?;
    let access_token = token_service
        .access_token(&token_set, time::OffsetDateTime::now_utc())
        .await?;
    if let Some(updated_token_set) = access_token.updated_token_set {
        shared
            .secret_store
            .update(secret_ref, &updated_token_set.encode()?)?;
    }

    Ok(access_token.token)
}

fn imap_adapter_error(error: ImapAdapterError) -> ServiceError {
    match error {
        ImapAdapterError::MissingTransport
        | ImapAdapterError::MissingSmtpTransport
        | ImapAdapterError::MissingUsername
        | ImapAdapterError::MissingSmtpSenderEmail
        | ImapAdapterError::MissingSecret
        | ImapAdapterError::InvalidMailboxName(_)
        | ImapAdapterError::MissingSelectData(_)
        | ImapAdapterError::UidValidityMismatch { .. }
        | ImapAdapterError::MissingFetchData(_)
        | ImapAdapterError::InvalidUidSequence(_)
        | ImapAdapterError::InvalidModSeq(_)
        | ImapAdapterError::InvalidKeywordFlag { .. }
        | ImapAdapterError::MissingMessageLocation(_)
        | ImapAdapterError::InvalidBlobId(_)
        | ImapAdapterError::ParseMessageHeaders
        | ImapAdapterError::ParseMessageBody
        | ImapAdapterError::MissingAttachment { .. }
        | ImapAdapterError::InvalidSmtpAddress { .. }
        | ImapAdapterError::BuildSmtpMessage(_) => GatewayError::Rejected(error.to_string()).into(),
        ImapAdapterError::Client(message) | ImapAdapterError::Smtp(message) => {
            GatewayError::Network(message).into()
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

    /// Update the running sync progress, setting account status to Syncing while present.
    async fn set_sync_progress(&self, account_id: &AccountId, progress: Option<SyncProgress>) {
        let mut current = self.runtime_overview(account_id).await;
        match progress {
            Some(progress) => {
                if !matches!(progress.stage, SyncProgressStage::Connecting)
                    && !matches!(current.status, AccountStatus::Syncing)
                {
                    return;
                }
                current.sync_progress = Some(progress);
                current.status = AccountStatus::Syncing;
            }
            None => {
                current.sync_progress = None;
            }
        }
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
        current.sync_progress = None;
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
        current.sync_progress = None;
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
        if previous.as_ref().map(|item| &item.status) != Some(&overview.status)
            || previous.as_ref().map(|item| &item.sync_progress) != Some(&overview.sync_progress)
        {
            if let Ok(event) = self.store.append_event(
                account_id,
                EVENT_TOPIC_ACCOUNT_STATUS_CHANGED,
                None,
                None,
                json!({
                    "status": &overview.status,
                    "push": &overview.push,
                    "lastSyncAt": overview.last_sync_at,
                    "lastSyncError": overview.last_sync_error,
                    "lastSyncErrorCode": overview.last_sync_error_code,
                    "syncProgress": overview.sync_progress,
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
