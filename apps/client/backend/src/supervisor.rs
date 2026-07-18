//! Per-account runtimes: each enabled account runs under a supervisor that
//! owns its provider connection and decides when it syncs — on push, on an
//! interval fallback, on explicit request, and when a scheduled send comes
//! due. The supervisor keeps the push connection alive with backoff and
//! reconnect, reports account health as queryable status, and recovers from
//! provider failures by degrading to polling rather than stopping. A
//! watchdog restarts a panicked or unexpectedly-exited runtime under a
//! bounded backoff, then halts the account with a truthful status.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::{future::pending, StreamExt};
use posthaste_domain_model::{
    AccountId, AccountRuntimeOverview, AccountSettings, AccountStatus, DomainEvent, GatewayError,
    PushNotification, PushStatus, RemoteObservationPolicy, ServiceError, SyncMode, SyncTrigger,
    EVENT_TOPIC_ACCOUNT_STATUS_CHANGED, EVENT_TOPIC_PUSH_CONNECTED, EVENT_TOPIC_PUSH_DISCONNECTED,
};
use posthaste_domain_service::{MailService, MailStore, PushStreamEvent, SecretStore};
use posthaste_observability::{events, ph_debug, ph_error, ph_info, ph_warn};
use serde_json::json;
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio::task::{AbortHandle, JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::{info_span, Instrument};

use crate::backfill::{
    process_backfill_batch, AUTOMATION_BACKFILL_BATCH_SIZE, AUTOMATION_BACKFILL_DRAIN_DELAY,
    AUTOMATION_BACKFILL_INITIAL_DELAY, AUTOMATION_BACKFILL_INTERVAL,
};
use crate::event_bus::EventBus;
use crate::gateway::{build_connection, ConnectionState};

/// Default provider poll interval: the safety net under push.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Snooze scheduler tick: local-store-only due-check returning snoozed
/// messages to the inbox.
const SNOOZE_INITIAL_DELAY: Duration = Duration::from_secs(30);
const SNOOZE_INTERVAL: Duration = Duration::from_secs(60);

/// Scheduled-send (undo-send / send-later) tick: a cheap indexed point probe
/// per tick; only a DUE send triggers a flush sync. Short because undo-send's
/// default hold is ~10s and the send should fire promptly once the window
/// closes (worst added latency = one interval).
const SCHEDULED_SEND_INITIAL_DELAY: Duration = Duration::from_secs(5);
const SCHEDULED_SEND_INTERVAL: Duration = Duration::from_secs(5);

/// Backstop budget for a full sync cycle inside a select-loop arm. Provider
/// calls carry their own tighter deadlines; this bound only fires when a call
/// path outside those envelopes hangs outright, so the loop degrades the
/// account and continues instead of wedging.
const ARM_BUDGET_SYNC: Duration = Duration::from_secs(300);

/// Backstop budget for one snooze/scheduled-send probe (local store work).
const ARM_BUDGET_TICK: Duration = Duration::from_secs(30);

/// Backstop budget for one automation-backfill batch
/// (`AUTOMATION_BACKFILL_BATCH_SIZE` provider round trips per tick).
const ARM_BUDGET_BACKFILL: Duration = Duration::from_secs(120);

/// A faulting account runtime is restarted at most this many times under
/// bounded backoff; the failure that would require one more restart halts it
/// with a truthful status.
const WATCHDOG_MAX_RESTARTS: u32 = 3;

/// An incarnation that stays healthy for at least this long resets the
/// restart budget — a fault after a sustained-healthy run is a fresh
/// incident, not part of a restart storm.
const WATCHDOG_HEALTHY_RESET_AFTER: Duration = Duration::from_secs(60);

/// Watchdog restart backoff: jittered exponential.
const WATCHDOG_BACKOFF_BASE: Duration = Duration::from_millis(500);
const WATCHDOG_BACKOFF_FACTOR: f64 = 2.0;
const WATCHDOG_BACKOFF_CAP: Duration = Duration::from_secs(30);

/// Deadline for a single cooperative per-account stop before the escalation
/// aborts the incarnation and its watchdog.
const PER_ACCOUNT_STOP_DEADLINE: Duration = Duration::from_secs(3);

/// Capacity of each account runtime's command channel.
const COMMAND_CHANNEL_CAPACITY: usize = 32;

/// Manages per-account async runtimes: connection lifecycle, sync triggers,
/// push stream consumption, and runtime status tracking.
pub struct AccountSupervisor {
    shared: Arc<SupervisorShared>,
    runtimes: RwLock<HashMap<String, ManagedRuntime>>,
    /// Supervisor-level cancellation token. Each account runtime's token is
    /// a child of this one, so `stop_all` cancels every account with a
    /// single `cancel()`; per-account stops cancel only their child.
    root_cancel: CancellationToken,
}

/// Shared state across all account runtimes.
struct SupervisorShared {
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    secret_store: Arc<dyn SecretStore>,
    events: EventBus,
    gateways: RwLock<HashMap<String, posthaste_domain_service::SharedGateway>>,
    runtime_overviews: RwLock<HashMap<String, AccountRuntimeOverview>>,
    runtime_generations: RwLock<HashMap<String, u64>>,
    known_accounts: RwLock<HashSet<String>>,
    account_count: AtomicUsize,
    poll_interval: Duration,
}

/// Coordinates fire-and-forget sync triggers between the supervisor and the
/// per-account runtime task. When the runtime is already executing a sync
/// cycle, additional trigger-only requests are coalesced into a single
/// pending trigger rather than enqueueing a full sync for each request: one
/// cycle drains all pending local-first operations, so a burst of mutations
/// never produces one provider sync per mutation.
struct SyncTriggerState {
    /// The `active` claim flag and coalesced `pending` trigger are guarded
    /// together so the trigger source and the runtime drain loop make one
    /// atomic decision — a trigger can neither be stranded between the drain
    /// clearing the flag and taking `pending`, nor can N concurrent
    /// idle-window triggers each enqueue a redundant cycle.
    inner: tokio::sync::Mutex<SyncCoalesceState>,
    sync_cycle_count: AtomicUsize,
}

#[derive(Default)]
struct SyncCoalesceState {
    /// True from the instant a cycle is admitted (claimed by a trigger or
    /// begun by a directly-driven cycle) until it — and any coalesced
    /// follow-up — fully finishes.
    active: bool,
    /// The most recent trigger that arrived while a cycle was admitted.
    pending: Option<SyncTrigger>,
}

impl SyncTriggerState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: tokio::sync::Mutex::new(SyncCoalesceState::default()),
            sync_cycle_count: AtomicUsize::new(0),
        })
    }

    /// Under one lock: if a cycle is already admitted, coalesce this trigger
    /// into the pending follow-up and return `true` (the caller must not
    /// enqueue a new cycle). Otherwise atomically claim the cycle — set
    /// `active` here, before the caller enqueues — and return `false`.
    async fn claim_or_coalesce(&self, trigger: SyncTrigger) -> bool {
        let mut inner = self.inner.lock().await;
        if inner.active {
            inner.pending = Some(trigger);
            true
        } else {
            inner.active = true;
            false
        }
    }

    /// Mark the runtime as entering a sync cycle and bump the cycle counter.
    /// Idempotent on `active` for the claim path; the set for directly-driven
    /// cycles (poll / push / manual / startup).
    async fn begin_cycle(&self) {
        self.inner.lock().await.active = true;
        self.sync_cycle_count.fetch_add(1, Ordering::SeqCst);
    }

    /// Finish the current cycle. If a trigger was coalesced while it ran,
    /// take it (keeping `active` set so the caller runs a follow-up cycle);
    /// otherwise clear `active` and return `None`.
    async fn finish_cycle_take_pending(&self) -> Option<SyncTrigger> {
        let mut inner = self.inner.lock().await;
        match inner.pending.take() {
            Some(trigger) => Some(trigger),
            None => {
                inner.active = false;
                None
            }
        }
    }

    /// Clear the coalescing state for a fresh incarnation (watchdog restart)
    /// or after an arm-budget timeout dropped a cycle mid-flight — without
    /// this, `active` would stay stuck true and every later trigger would
    /// coalesce into a dead cycle instead of enqueueing a fresh one.
    async fn reset(&self) {
        let mut inner = self.inner.lock().await;
        inner.active = false;
        inner.pending = None;
    }

    fn sync_cycle_count(&self) -> usize {
        self.sync_cycle_count.load(Ordering::SeqCst)
    }
}

/// A supervised account: the watchdog task plus the swappable handles the
/// watchdog rewires each time it restarts the underlying incarnation. The
/// command channel's receiver is consumed by each incarnation, so a restart
/// mints a fresh channel; the live sender lives behind a slot.
struct ManagedRuntime {
    command_tx: Arc<StdMutex<Option<mpsc::Sender<RuntimeCommand>>>>,
    sync_state: Arc<SyncTriggerState>,
    cancel: CancellationToken,
    incarnation_abort: Arc<StdMutex<Option<AbortHandle>>>,
    monitor: JoinHandle<()>,
}

/// Commands sent to a running account runtime.
enum RuntimeCommand {
    Trigger {
        trigger: SyncTrigger,
        mode: SyncMode,
        reply: oneshot::Sender<Result<usize, ServiceError>>,
    },
    TriggerOnly {
        trigger: SyncTrigger,
    },
}

impl AccountSupervisor {
    /// Create a supervisor over the shared service graph.
    pub fn new(
        service: Arc<MailService>,
        store: Arc<dyn MailStore>,
        secret_store: Arc<dyn SecretStore>,
        events: EventBus,
        poll_interval: Duration,
    ) -> Self {
        Self {
            shared: Arc::new(SupervisorShared {
                service,
                store,
                secret_store,
                events,
                gateways: RwLock::new(HashMap::new()),
                runtime_overviews: RwLock::new(HashMap::new()),
                runtime_generations: RwLock::new(HashMap::new()),
                known_accounts: RwLock::new(HashSet::new()),
                account_count: AtomicUsize::new(0),
                poll_interval,
            }),
            runtimes: RwLock::new(HashMap::new()),
            root_cancel: CancellationToken::new(),
        }
    }

    /// Start (or restart) the async runtime for an account. Stops any
    /// existing runtime first. Disabled accounts get a `Disabled` status
    /// without spawning a task.
    pub async fn start_account(&self, account: &AccountSettings) {
        self.stop_account(&account.id).await;
        self.shared.register_account(&account.id).await;
        if !account.enabled {
            ph_info!(
                events::SUPERVISOR_ACCOUNT_DISABLED,
                account_id = %account.id,
                "account disabled, skipping runtime"
            );
            // Advance the generation so a late write from the just-stopped
            // task cannot revive a status over the Disabled overview.
            self.shared.next_runtime_generation(&account.id).await;
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

        ph_info!(
            events::SUPERVISOR_ACCOUNT_RUNTIME_STARTED,
            account_id = %account.id,
            driver = ?account.driver,
            "starting account runtime"
        );

        let cancel = self.root_cancel.child_token();
        let sync_state = SyncTriggerState::new();
        let command_slot: Arc<StdMutex<Option<mpsc::Sender<RuntimeCommand>>>> =
            Arc::new(StdMutex::new(None));
        let incarnation_abort: Arc<StdMutex<Option<AbortHandle>>> = Arc::new(StdMutex::new(None));

        let spawn = build_incarnation_spawner(
            self.shared.clone(),
            account.clone(),
            sync_state.clone(),
            command_slot.clone(),
            incarnation_abort.clone(),
        );

        let first = spawn(cancel.clone());
        let monitor = tokio::spawn(run_watchdog(
            account.id.clone(),
            self.shared.clone(),
            cancel.clone(),
            spawn,
            first,
        ));

        self.runtimes.write().await.insert(
            account.id.to_string(),
            ManagedRuntime {
                command_tx: command_slot,
                sync_state,
                cancel,
                incarnation_abort,
                monitor,
            },
        );
    }

    /// Stop every account runtime, bounded by `deadline`. Cooperative-first:
    /// cancelling the root token trips every account's cancelled arm at
    /// once; then each watchdog is joined under the shared deadline, and
    /// only a straggler is escalated to `abort()`.
    pub async fn stop_all(&self, deadline: Duration) {
        self.root_cancel.cancel();
        let entries: Vec<(String, ManagedRuntime)> = self.runtimes.write().await.drain().collect();
        let deadline_at = tokio::time::Instant::now() + deadline;
        for (account_id, runtime) in entries {
            let ManagedRuntime {
                cancel,
                incarnation_abort,
                monitor,
                ..
            } = runtime;
            cancel.cancel();
            let remaining = deadline_at.saturating_duration_since(tokio::time::Instant::now());
            join_or_escalate(&account_id, monitor, &incarnation_abort, remaining).await;
            self.shared
                .remove_gateway(&AccountId::from(account_id.as_str()))
                .await;
        }
    }

    /// Stop one account's runtime cooperatively and remove its gateway.
    pub async fn stop_account(&self, account_id: &AccountId) {
        let removed = self.runtimes.write().await.remove(account_id.as_str());
        if let Some(runtime) = removed {
            ph_info!(
                events::SUPERVISOR_ACCOUNT_RUNTIME_STOPPED,
                account_id = %account_id,
                "stopping account runtime"
            );
            let ManagedRuntime {
                cancel,
                incarnation_abort,
                monitor,
                ..
            } = runtime;
            cancel.cancel();
            join_or_escalate(
                account_id.as_str(),
                monitor,
                &incarnation_abort,
                PER_ACCOUNT_STOP_DEADLINE,
            )
            .await;
        }
        self.shared.remove_gateway(account_id).await;
    }

    /// Stop the runtime and clear runtime overview state for a deleted
    /// account.
    pub async fn remove_account(&self, account_id: &AccountId) {
        ph_info!(
            events::SUPERVISOR_ACCOUNT_REMOVED,
            account_id = %account_id,
            "removing account"
        );
        self.stop_account(account_id).await;
        self.shared.next_runtime_generation(account_id).await;
        self.shared.unregister_account(account_id).await;
        self.shared
            .runtime_overviews
            .write()
            .await
            .remove(account_id.as_str());
    }

    /// Send a manual sync trigger to the account runtime and await its
    /// result (the number of domain events the cycle produced).
    pub async fn sync_account(&self, account_id: &AccountId) -> Result<usize, ServiceError> {
        self.sync_account_with_mode(account_id, SyncMode::Incremental)
            .await
    }

    /// Send a manual sync trigger with an explicit mode and await its result.
    pub async fn sync_account_with_mode(
        &self,
        account_id: &AccountId,
        mode: SyncMode,
    ) -> Result<usize, ServiceError> {
        let command_tx = self.live_command_tx(account_id).await?;
        let (reply_tx, reply_rx) = oneshot::channel();
        command_tx
            .send(RuntimeCommand::Trigger {
                trigger: SyncTrigger::Manual,
                mode,
                reply: reply_tx,
            })
            .await
            .map_err(|_| GatewayError::Unavailable(account_id.to_string()))?;
        reply_rx
            .await
            .map_err(|_| ServiceError::from(GatewayError::Unavailable(account_id.to_string())))?
    }

    /// Request a runtime sync without waiting for completion. If the runtime
    /// is already inside a sync cycle, the trigger is coalesced into a
    /// single pending follow-up instead of enqueueing another full sync.
    pub async fn trigger_account_sync(
        &self,
        account_id: &AccountId,
        trigger: SyncTrigger,
    ) -> Result<(), ServiceError> {
        // Clone the live command sender + sync-state handle, then release
        // the runtimes read lock before awaiting.
        let (command_tx, sync_state) = {
            let runtimes = self.runtimes.read().await;
            let runtime = runtimes
                .get(account_id.as_str())
                .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()))?;
            let command_tx = runtime
                .command_tx
                .lock()
                .expect("command slot poisoned")
                .clone();
            (command_tx, runtime.sync_state.clone())
        };
        let command_tx =
            command_tx.ok_or_else(|| GatewayError::Unavailable(account_id.to_string()))?;

        // Reserve a command slot before the claim so a trigger is never
        // dropped if the runtime stops between the check and the send; the
        // reserved slot is released when the trigger coalesces.
        let permit = command_tx
            .reserve()
            .await
            .map_err(|_| GatewayError::Unavailable(account_id.to_string()))?;

        if sync_state.claim_or_coalesce(trigger.clone()).await {
            ph_debug!(
                events::SUPERVISOR_SYNC_TRIGGER_COALESCED,
                account_id = %account_id,
                trigger = trigger.as_str(),
                "sync trigger coalesced while runtime is already syncing"
            );
            drop(permit);
            return Ok(());
        }
        permit.send(RuntimeCommand::TriggerOnly { trigger });
        Ok(())
    }

    /// Number of sync cycles the account runtime has executed since it was
    /// started (observability/test seam).
    pub async fn sync_cycle_count(&self, account_id: &AccountId) -> usize {
        let runtimes = self.runtimes.read().await;
        runtimes
            .get(account_id.as_str())
            .map(|runtime| runtime.sync_state.sync_cycle_count())
            .unwrap_or(0)
    }

    /// Current runtime status snapshot for an account.
    pub async fn runtime_overview(&self, account_id: &AccountId) -> AccountRuntimeOverview {
        self.shared.runtime_overview(account_id).await
    }

    /// Runtime status snapshots for every known account (the accounts-health
    /// query surface).
    pub async fn runtime_overviews(&self) -> HashMap<String, AccountRuntimeOverview> {
        self.shared.runtime_overviews.read().await.clone()
    }

    /// Number of accounts known to the supervisor (enabled or not).
    pub fn account_count(&self) -> usize {
        self.shared.account_count.load(Ordering::SeqCst)
    }

    /// The live gateway for an account, if its runtime is connected.
    pub async fn gateway(
        &self,
        account_id: &AccountId,
    ) -> Result<posthaste_domain_service::SharedGateway, ServiceError> {
        self.shared.gateway(account_id).await
    }

    async fn live_command_tx(
        &self,
        account_id: &AccountId,
    ) -> Result<mpsc::Sender<RuntimeCommand>, ServiceError> {
        let slot = {
            let runtimes = self.runtimes.read().await;
            runtimes
                .get(account_id.as_str())
                .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()))?
                .command_tx
                .clone()
        };
        let sender = slot.lock().expect("command slot poisoned").clone();
        sender.ok_or_else(|| GatewayError::Unavailable(account_id.to_string()).into())
    }
}

impl SupervisorShared {
    async fn gateway(
        &self,
        account_id: &AccountId,
    ) -> Result<posthaste_domain_service::SharedGateway, ServiceError> {
        self.gateways
            .read()
            .await
            .get(account_id.as_str())
            .cloned()
            .ok_or_else(|| GatewayError::Unavailable(account_id.to_string()).into())
    }

    async fn set_gateway(
        &self,
        account_id: &AccountId,
        gateway: posthaste_domain_service::SharedGateway,
    ) {
        self.gateways
            .write()
            .await
            .insert(account_id.to_string(), gateway);
    }

    async fn remove_gateway(&self, account_id: &AccountId) {
        self.gateways.write().await.remove(account_id.as_str());
    }

    async fn register_account(&self, account_id: &AccountId) {
        let count = {
            let mut known = self.known_accounts.write().await;
            known.insert(account_id.to_string());
            known.len()
        };
        self.account_count.store(count, Ordering::SeqCst);
    }

    async fn unregister_account(&self, account_id: &AccountId) {
        let count = {
            let mut known = self.known_accounts.write().await;
            known.remove(account_id.as_str());
            known.len()
        };
        self.account_count.store(count, Ordering::SeqCst);
    }

    /// Mint the next incarnation generation for an account. Async tasks
    /// capture the generation they were spawned with; any delayed status
    /// write from an older generation is dropped, so a stopped/restarted
    /// task cannot overwrite the current runtime state.
    async fn next_runtime_generation(&self, account_id: &AccountId) -> u64 {
        let mut generations = self.runtime_generations.write().await;
        let generation = generations
            .get(account_id.as_str())
            .copied()
            .unwrap_or(0)
            .saturating_add(1);
        generations.insert(account_id.to_string(), generation);
        generation
    }

    /// Broadcast committed-write events on the bus (bumping the generation).
    fn publish_events(&self, batch: &[DomainEvent]) {
        self.events.publish(batch);
    }

    async fn runtime_overview(&self, account_id: &AccountId) -> AccountRuntimeOverview {
        self.runtime_overviews
            .read()
            .await
            .get(account_id.as_str())
            .cloned()
            .unwrap_or_default()
    }

    async fn set_runtime_overview(&self, account_id: &AccountId, overview: AccountRuntimeOverview) {
        self.update_runtime_overview(account_id, None, move |current| {
            *current = overview;
            true
        })
        .await;
    }

    async fn set_runtime_overview_for_generation(
        &self,
        account_id: &AccountId,
        generation: u64,
        overview: AccountRuntimeOverview,
    ) {
        self.update_runtime_overview(account_id, Some(generation), move |current| {
            *current = overview;
            true
        })
        .await;
    }

    /// Mark the account as syncing while a cycle runs.
    async fn mark_syncing(&self, account_id: &AccountId, generation: u64) {
        self.update_runtime_overview(account_id, Some(generation), |current| {
            if matches!(current.status, AccountStatus::Syncing) {
                return false;
            }
            current.status = AccountStatus::Syncing;
            true
        })
        .await;
    }

    /// Record a successful sync: status `Ready`, error cleared, timestamp.
    async fn mark_sync_success(&self, account_id: &AccountId, generation: u64) {
        self.update_runtime_overview(account_id, Some(generation), |current| {
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
            true
        })
        .await;
    }

    /// Record a sync failure: derive status from the error class, store the
    /// user-facing message and stable code (never the raw provider string).
    async fn mark_sync_failure(
        &self,
        account_id: &AccountId,
        generation: u64,
        error: &ServiceError,
    ) {
        let presented = error.user_facing();
        self.update_runtime_overview(account_id, Some(generation), |current| {
            current.status = match error {
                ServiceError::Gateway(GatewayError::Auth) => AccountStatus::AuthError,
                ServiceError::Gateway(GatewayError::Network(_))
                | ServiceError::Gateway(GatewayError::Unavailable(_))
                | ServiceError::Secret(_) => AccountStatus::Offline,
                _ => AccountStatus::Degraded,
            };
            current.last_sync_error = Some(presented.message);
            current.last_sync_error_code = Some(presented.code.to_string());
            current.sync_progress = None;
            if !matches!(current.push, PushStatus::Unsupported | PushStatus::Disabled) {
                current.push = PushStatus::Reconnecting;
            }
            true
        })
        .await;
    }

    /// Update only the push status, preserving other overview fields.
    async fn set_push_status(&self, account_id: &AccountId, generation: u64, push: PushStatus) {
        self.update_runtime_overview(account_id, Some(generation), move |current| {
            if current.push == push {
                return false;
            }
            current.push = push;
            true
        })
        .await;
    }

    /// Handle a push stream disconnect: persist + broadcast the event and
    /// set push status to `Reconnecting`.
    async fn handle_push_disconnect(&self, account_id: &AccountId, generation: u64, message: &str) {
        match self.store.append_event(
            account_id,
            EVENT_TOPIC_PUSH_DISCONNECTED,
            None,
            None,
            json!({ "message": message }),
        ) {
            Ok(event) => self.publish_events(std::slice::from_ref(&event)),
            Err(error) => ph_warn!(
                events::SUPERVISOR_ACCOUNT_STATUS_PERSIST_FAILED,
                account_id = %account_id,
                topic = EVENT_TOPIC_PUSH_DISCONNECTED,
                error = %error,
                "failed to persist push disconnect event"
            ),
        }
        self.set_push_status(account_id, generation, PushStatus::Reconnecting)
            .await;
    }

    /// Mark push terminally unavailable for this account: a structurally
    /// broken transport that repeated reconnects cannot fix. Push status
    /// goes `Unsupported` (the truthful terminal state); the account's sync
    /// status is left untouched — the poll keeps mail fresh.
    async fn mark_push_terminal(
        &self,
        account_id: &AccountId,
        generation: u64,
        transport: &str,
        reason: &str,
    ) {
        let poll_secs = self.poll_interval.as_secs();
        self.update_runtime_overview(account_id, Some(generation), move |current| {
            if current.push == PushStatus::Unsupported {
                return false;
            }
            current.push = PushStatus::Unsupported;
            current.last_sync_error = Some(format!(
                "push unavailable via {transport}: {reason}; polling every {poll_secs}s instead"
            ));
            current.last_sync_error_code = Some("push_terminal".to_string());
            true
        })
        .await;
    }

    /// Mark an account impaired by an internal runtime fault (panic or
    /// unexpected exit) that the watchdog is about to retry. Written
    /// unconditionally: the faulted incarnation is already dead and the next
    /// incarnation's own startup writes supersede this.
    async fn mark_account_faulted(&self, account_id: &AccountId, attempt: u32, reason: &str) {
        self.update_runtime_overview(account_id, None, |current| {
            current.status = AccountStatus::Degraded;
            current.last_sync_error = Some(format!(
                "account runtime fault (restart {attempt}/{WATCHDOG_MAX_RESTARTS}): {reason}"
            ));
            current.last_sync_error_code = Some("runtime_fault".to_string());
            current.sync_progress = None;
            if !matches!(current.push, PushStatus::Unsupported | PushStatus::Disabled) {
                current.push = PushStatus::Reconnecting;
            }
            true
        })
        .await;
    }

    /// Mark an account halted after the watchdog exhausted its restart
    /// budget. The runtime is no longer running, so `Offline` is the
    /// truthful state; push is `Disabled` because nothing is left to
    /// reconnect it.
    async fn mark_account_halted(&self, account_id: &AccountId, reason: &str) {
        self.update_runtime_overview(account_id, None, |current| {
            current.status = AccountStatus::Offline;
            current.push = PushStatus::Disabled;
            current.last_sync_error = Some(format!(
                "account runtime halted after {WATCHDOG_MAX_RESTARTS} failed restarts: {reason}"
            ));
            current.last_sync_error_code = Some("runtime_halted".to_string());
            current.sync_progress = None;
            true
        })
        .await;
    }

    /// Record that a select-loop arm's bounded call elapsed before
    /// completing: the account degrades but the loop keeps running — the
    /// watchdog owns lifecycle, not this per-arm backstop.
    async fn mark_arm_timeout(
        &self,
        account_id: &AccountId,
        generation: u64,
        arm: &'static str,
        budget: Duration,
    ) {
        self.update_runtime_overview(account_id, Some(generation), |current| {
            current.status = AccountStatus::Degraded;
            current.last_sync_error = Some(format!(
                "supervisor arm '{arm}' exceeded its {budget:?} budget (provider/store call hung)"
            ));
            current.last_sync_error_code = Some("arm_timeout".to_string());
            current.sync_progress = None;
            true
        })
        .await;
    }

    /// Atomically read-modify-write an account's runtime overview under the
    /// overviews write lock, guarded by the incarnation generation when
    /// given (a stale task's write is dropped). Persists + broadcasts an
    /// account-status-changed event when the visible status changes, and
    /// push connect/disconnect events on push transitions.
    async fn update_runtime_overview(
        &self,
        account_id: &AccountId,
        generation: Option<u64>,
        update: impl FnOnce(&mut AccountRuntimeOverview) -> bool,
    ) {
        let generations = self.runtime_generations.read().await;
        if let Some(expected) = generation {
            let Some(current) = generations.get(account_id.as_str()) else {
                return;
            };
            if *current != expected {
                return;
            }
        }

        let mut overviews = self.runtime_overviews.write().await;
        let previous = overviews.get(account_id.as_str()).cloned();
        let mut overview = previous.clone().unwrap_or_default();
        if !update(&mut overview) {
            return;
        }

        let mut side_effects = Vec::new();
        if previous.as_ref().map(|item| &item.status) != Some(&overview.status)
            || previous.as_ref().map(|item| &item.push) != Some(&overview.push)
            || previous.as_ref().map(|item| &item.last_sync_error_code)
                != Some(&overview.last_sync_error_code)
        {
            match self.store.append_event(
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
                }),
            ) {
                Ok(event) => side_effects.push(event),
                Err(error) => {
                    ph_warn!(
                        events::SUPERVISOR_ACCOUNT_STATUS_PERSIST_FAILED,
                        account_id = %account_id,
                        topic = EVENT_TOPIC_ACCOUNT_STATUS_CHANGED,
                        error = %error,
                        "failed to persist account status change event"
                    );
                    return;
                }
            }
        }

        match (previous.as_ref().map(|item| &item.push), &overview.push) {
            (Some(PushStatus::Connected), PushStatus::Connected) => {}
            (_, PushStatus::Connected) => match self.store.append_event(
                account_id,
                EVENT_TOPIC_PUSH_CONNECTED,
                None,
                None,
                json!({}),
            ) {
                Ok(event) => side_effects.push(event),
                Err(error) => ph_warn!(
                    events::SUPERVISOR_ACCOUNT_STATUS_PERSIST_FAILED,
                    account_id = %account_id,
                    topic = EVENT_TOPIC_PUSH_CONNECTED,
                    error = %error,
                    "failed to persist push connected event"
                ),
            },
            (Some(PushStatus::Connected), _) => match self.store.append_event(
                account_id,
                EVENT_TOPIC_PUSH_DISCONNECTED,
                None,
                None,
                json!({}),
            ) {
                Ok(event) => side_effects.push(event),
                Err(error) => ph_warn!(
                    events::SUPERVISOR_ACCOUNT_STATUS_PERSIST_FAILED,
                    account_id = %account_id,
                    topic = EVENT_TOPIC_PUSH_DISCONNECTED,
                    error = %error,
                    "failed to persist push disconnected event"
                ),
            },
            _ => {}
        }

        overviews.insert(account_id.to_string(), overview);
        drop(overviews);
        drop(generations);
        self.publish_events(&side_effects);
    }

    /// Wall-clock "now" in UNIX seconds, anchored against a monotonic
    /// instant taken at first use, so a backward clock correction can never
    /// make an already-due snooze or scheduled send look not-yet-due.
    fn monotonic_now_secs() -> i64 {
        static ANCHOR: OnceLock<(Instant, SystemTime)> = OnceLock::new();
        let &(anchor_instant, anchor_wall) =
            ANCHOR.get_or_init(|| (Instant::now(), SystemTime::now()));
        let elapsed = Instant::now().saturating_duration_since(anchor_instant);
        (anchor_wall + elapsed)
            .duration_since(UNIX_EPOCH)
            .map(|delta| delta.as_secs() as i64)
            .unwrap_or(0)
    }
}

/// The (re)spawn factory the watchdog calls to (re)start an account
/// incarnation: mints a fresh command channel, publishes the live sender +
/// abort handle into the shared slots, spawns the runtime task, and returns
/// its `JoinHandle`.
type SpawnIncarnation = Box<dyn Fn(CancellationToken) -> JoinHandle<()> + Send + Sync>;

fn build_incarnation_spawner(
    shared: Arc<SupervisorShared>,
    account: AccountSettings,
    sync_state: Arc<SyncTriggerState>,
    command_slot: Arc<StdMutex<Option<mpsc::Sender<RuntimeCommand>>>>,
    incarnation_abort: Arc<StdMutex<Option<AbortHandle>>>,
) -> SpawnIncarnation {
    Box::new(move |cancel: CancellationToken| {
        let (command_tx, command_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
        *command_slot.lock().expect("command slot poisoned") = Some(command_tx);
        let shared = shared.clone();
        let account = account.clone();
        let sync_state = sync_state.clone();
        let span = info_span!("supervisor.runtime", account_id = %account.id);
        let handle = tokio::spawn(
            async move {
                // Fresh incarnation: clear coalescing state a panicked cycle
                // may have left, and take a new generation so a late write
                // from the prior incarnation is dropped.
                sync_state.reset().await;
                let generation = shared.next_runtime_generation(&account.id).await;
                run_account_runtime(shared, account, generation, command_rx, sync_state, cancel)
                    .await;
            }
            .instrument(span),
        );
        *incarnation_abort.lock().expect("abort slot poisoned") = Some(handle.abort_handle());
        handle
    })
}

/// A cheap, dependency-free uniform-ish value in `[0, 1)` for full-jitter
/// decorrelation of restart backoff. Not cryptographic.
fn jitter_unit() -> f64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or(0);
    f64::from(nanos % 1_000_000) / 1_000_000.0
}

/// Full-jitter exponential backoff delay for a 0-based attempt.
fn watchdog_backoff_delay(attempt: u32) -> Duration {
    let ceiling = (WATCHDOG_BACKOFF_BASE.as_secs_f64()
        * WATCHDOG_BACKOFF_FACTOR.powi(attempt.min(16) as i32))
    .min(WATCHDOG_BACKOFF_CAP.as_secs_f64());
    Duration::from_secs_f64(ceiling * jitter_unit())
}

/// Extract a human-readable message from a captured panic payload.
fn panic_payload_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

/// The supervisor-owned watchdog for one account: observes the incarnation
/// `JoinHandle` so a panic or unexpected exit is surfaced instead of
/// silently swallowed, restarts under the bounded policy, then halts the
/// account with a truthful status. Returns when the account is cancelled or
/// halted.
async fn run_watchdog(
    account_id: AccountId,
    shared: Arc<SupervisorShared>,
    cancel: CancellationToken,
    spawn: SpawnIncarnation,
    mut current: JoinHandle<()>,
) {
    let mut restarts: u32 = 0;
    loop {
        let started = tokio::time::Instant::now();
        let outcome = tokio::select! {
            biased;
            () = cancel.cancelled() => {
                // Cooperative stop: let the incarnation finish its own
                // graceful exit.
                let _ = current.await;
                return;
            }
            result = &mut current => result,
        };

        let reason = match outcome {
            Ok(()) => {
                if cancel.is_cancelled() {
                    return;
                }
                ph_warn!(
                    events::SUPERVISOR_ACCOUNT_EXITED_UNEXPECTEDLY,
                    account_id = %account_id,
                    "account runtime exited unexpectedly"
                );
                "runtime exited unexpectedly".to_string()
            }
            Err(join_error) if join_error.is_panic() => {
                let message = panic_payload_message(join_error.into_panic());
                ph_error!(
                    events::SUPERVISOR_ACCOUNT_PANICKED,
                    account_id = %account_id,
                    payload = %message,
                    "account runtime panicked"
                );
                message
            }
            Err(_join_error) => {
                // A cancelled/aborted JoinError only arises from the stop
                // escalation (which cancels first) — a cooperative stop.
                return;
            }
        };

        if started.elapsed() >= WATCHDOG_HEALTHY_RESET_AFTER {
            restarts = 0;
        }
        restarts += 1;

        if restarts > WATCHDOG_MAX_RESTARTS {
            shared.mark_account_halted(&account_id, &reason).await;
            ph_error!(
                events::SUPERVISOR_ACCOUNT_HALTED,
                account_id = %account_id,
                max_restarts = WATCHDOG_MAX_RESTARTS,
                "account runtime halted after exhausting its restart budget"
            );
            return;
        }

        shared
            .mark_account_faulted(&account_id, restarts, &reason)
            .await;
        let delay = watchdog_backoff_delay(restarts - 1);
        ph_warn!(
            events::SUPERVISOR_ACCOUNT_RESTARTING,
            account_id = %account_id,
            attempt = restarts,
            max_restarts = WATCHDOG_MAX_RESTARTS,
            delay_ms = delay.as_millis() as u64,
            "restarting account runtime after backoff"
        );
        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(delay) => {}
        }
        current = spawn(cancel.clone());
    }
}

/// Join the account's watchdog under `deadline`; only if it overruns do we
/// escalate to aborting the current incarnation and then the watchdog.
async fn join_or_escalate(
    account_id: &str,
    monitor: JoinHandle<()>,
    incarnation_abort: &Arc<StdMutex<Option<AbortHandle>>>,
    deadline: Duration,
) {
    let monitor_abort = monitor.abort_handle();
    if tokio::time::timeout(deadline, monitor).await.is_err() {
        if let Some(abort) = incarnation_abort
            .lock()
            .expect("abort slot poisoned")
            .as_ref()
        {
            abort.abort();
        }
        monitor_abort.abort();
        ph_warn!(
            events::SUPERVISOR_ACCOUNT_STOP_ESCALATED,
            account_id = %account_id,
            deadline_ms = deadline.as_millis() as u64,
            "account did not stop cooperatively within the deadline; escalated to abort"
        );
    }
}

fn sync_poll_interval(poll_interval: Duration) -> tokio::time::Interval {
    let mut interval =
        tokio::time::interval_at(tokio::time::Instant::now() + poll_interval, poll_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval
}

fn tick_interval(initial_delay: Duration, period: Duration) -> tokio::time::Interval {
    let mut interval =
        tokio::time::interval_at(tokio::time::Instant::now() + initial_delay, period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval
}

/// Main event loop for an account: polls on timer, push notifications,
/// scheduled-send and snooze ticks, and manual sync commands. Runs until
/// cancelled.
async fn run_account_runtime(
    shared: Arc<SupervisorShared>,
    account: AccountSettings,
    generation: u64,
    mut command_rx: mpsc::Receiver<RuntimeCommand>,
    sync_state: Arc<SyncTriggerState>,
    cancel: CancellationToken,
) {
    let account_id = account.id.clone();
    let mut connection = ConnectionState::default();
    let mut snooze_interval = tick_interval(SNOOZE_INITIAL_DELAY, SNOOZE_INTERVAL);
    let mut scheduled_send_interval =
        tick_interval(SCHEDULED_SEND_INITIAL_DELAY, SCHEDULED_SEND_INTERVAL);
    let mut backfill_interval = tick_interval(
        AUTOMATION_BACKFILL_INITIAL_DELAY,
        AUTOMATION_BACKFILL_INTERVAL,
    );

    shared
        .set_runtime_overview_for_generation(
            &account_id,
            generation,
            AccountRuntimeOverview {
                status: AccountStatus::Offline,
                push: PushStatus::Reconnecting,
                ..Default::default()
            },
        )
        .await;

    // Initial sync + gateway setup, bounded like an in-loop sync arm so a
    // hung provider cannot wedge the task before the loop starts.
    if tokio::time::timeout(
        ARM_BUDGET_SYNC,
        process_sync_trigger_with_state(
            &sync_state,
            &shared,
            &account,
            generation,
            SyncTriggerRequest::new(SyncTrigger::Startup, SyncMode::Incremental),
            &mut connection,
        ),
    )
    .await
    .is_err()
    {
        record_arm_timeout(
            &sync_state,
            &shared,
            &account_id,
            generation,
            "startup_sync",
            ARM_BUDGET_SYNC,
        )
        .await;
    }
    let mut interval = sync_poll_interval(shared.poll_interval);

    loop {
        let next_push = async {
            match connection.push_events_mut() {
                Some(stream) => stream.next().await,
                None => pending().await,
            }
        };

        tokio::select! {
            // Cooperative stop. In-flight work at the current await point is
            // dropped — store writes are transactional, so a dropped cycle
            // simply re-runs on the next start.
            () = cancel.cancelled() => {
                ph_info!(
                    events::SUPERVISOR_ACCOUNT_RUNTIME_STOPPED,
                    account_id = %account_id,
                    "account runtime cancelled; exiting loop"
                );
                break;
            }
            _ = interval.tick() => {
                if tokio::time::timeout(
                    ARM_BUDGET_SYNC,
                    process_sync_trigger_with_state(
                        &sync_state,
                        &shared,
                        &account,
                        generation,
                        SyncTriggerRequest::new(SyncTrigger::Poll, SyncMode::Incremental),
                        &mut connection,
                    ),
                ).await.is_err() {
                    record_arm_timeout(&sync_state, &shared, &account_id, generation, "poll_sync", ARM_BUDGET_SYNC).await;
                }
                interval = sync_poll_interval(shared.poll_interval);
            }
            _ = snooze_interval.tick() => {
                if tokio::time::timeout(
                    ARM_BUDGET_TICK,
                    handle_snooze_tick(&shared, &account_id),
                ).await.is_err() {
                    record_arm_timeout(&sync_state, &shared, &account_id, generation, "snooze", ARM_BUDGET_TICK).await;
                }
            }
            _ = scheduled_send_interval.tick() => {
                // Budgeted like a sync arm: the tick itself is a point
                // probe, but a due send triggers a full flush sync inline.
                if tokio::time::timeout(
                    ARM_BUDGET_SYNC,
                    handle_scheduled_send_tick(&sync_state, &shared, &account, generation, &mut connection),
                ).await.is_err() {
                    record_arm_timeout(&sync_state, &shared, &account_id, generation, "scheduled_send", ARM_BUDGET_SYNC).await;
                }
            }
            _ = backfill_interval.tick() => {
                match tokio::time::timeout(
                    ARM_BUDGET_BACKFILL,
                    handle_backfill_tick(&shared, &account_id, connection.gateway()),
                ).await {
                    // More work queued: come back after the short drain
                    // delay instead of a full interval — one bounded batch
                    // per pass keeps a big backfill interleaved with sync,
                    // push, and commands instead of starving them.
                    Ok(true) => backfill_interval.reset_after(AUTOMATION_BACKFILL_DRAIN_DELAY),
                    Ok(false) => {}
                    Err(_) => record_arm_timeout(&sync_state, &shared, &account_id, generation, "backfill", ARM_BUDGET_BACKFILL).await,
                }
            }
            Some(command) = command_rx.recv() => {
                match tokio::time::timeout(
                    ARM_BUDGET_SYNC,
                    handle_runtime_command(
                        &sync_state,
                        &shared,
                        &account,
                        generation,
                        &mut connection,
                        command,
                    ),
                ).await {
                    Ok(()) => interval = sync_poll_interval(shared.poll_interval),
                    Err(_) => record_arm_timeout(&sync_state, &shared, &account_id, generation, "command", ARM_BUDGET_SYNC).await,
                }
            }
            Some(event) = next_push => {
                match tokio::time::timeout(
                    ARM_BUDGET_SYNC,
                    handle_push_event(&sync_state, &shared, &account, generation, &mut connection, event),
                ).await {
                    Ok(true) => interval = sync_poll_interval(shared.poll_interval),
                    Ok(false) => {}
                    Err(_) => record_arm_timeout(&sync_state, &shared, &account_id, generation, "push_event", ARM_BUDGET_SYNC).await,
                }
            }
        }
    }
}

/// Log and degrade the account when a select-loop arm's bounded call
/// elapses; never breaks the caller's loop. Also resets the coalescer: the
/// dropped cycle's `finish_cycle_take_pending` never ran, so without the
/// reset every later trigger would coalesce into a dead cycle.
async fn record_arm_timeout(
    sync_state: &Arc<SyncTriggerState>,
    shared: &Arc<SupervisorShared>,
    account_id: &AccountId,
    generation: u64,
    arm: &'static str,
    budget: Duration,
) {
    sync_state.reset().await;
    ph_warn!(
        events::SUPERVISOR_ARM_TIMEOUT,
        account_id = %account_id,
        arm,
        "supervisor select-loop arm exceeded its bounded budget; account degraded, loop continues"
    );
    shared
        .mark_arm_timeout(account_id, generation, arm, budget)
        .await;
}

/// A single sync request.
struct SyncTriggerRequest {
    trigger: SyncTrigger,
    mode: SyncMode,
    reply: Option<oneshot::Sender<Result<usize, ServiceError>>>,
}

impl SyncTriggerRequest {
    fn new(trigger: SyncTrigger, mode: SyncMode) -> Self {
        Self {
            trigger,
            mode,
            reply: None,
        }
    }

    fn with_reply(
        trigger: SyncTrigger,
        mode: SyncMode,
        reply: oneshot::Sender<Result<usize, ServiceError>>,
    ) -> Self {
        Self {
            trigger,
            mode,
            reply: Some(reply),
        }
    }
}

/// Run a single sync cycle, keeping the coalescer informed. After the
/// requested cycle finishes, any trigger coalesced while it ran is drained
/// by running exactly one follow-up cycle.
async fn process_sync_trigger_with_state(
    sync_state: &Arc<SyncTriggerState>,
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    generation: u64,
    request: SyncTriggerRequest,
    connection: &mut ConnectionState,
) {
    let mut next = Some(request);
    while let Some(request) = next {
        sync_state.begin_cycle().await;
        process_sync_trigger(shared, account, generation, request, connection).await;
        next = sync_state
            .finish_cycle_take_pending()
            .await
            .map(|trigger| SyncTriggerRequest::new(trigger, SyncMode::Incremental));
    }
}

/// Execute one sync cycle: ensure connection, run the service's
/// flush → observe → retire cycle, publish events as they are produced, and
/// update runtime status. On failure, tears down the connection (the next
/// cycle reconnects) and records the error.
async fn process_sync_trigger(
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    generation: u64,
    request: SyncTriggerRequest,
    connection: &mut ConnectionState,
) {
    let SyncTriggerRequest {
        trigger,
        mode,
        reply,
    } = request;
    let account_id = account.id.clone();
    let started = Instant::now();
    ph_info!(
        events::SUPERVISOR_SYNC_STARTED,
        account_id = %account_id,
        trigger = trigger.as_str(),
        "sync started"
    );
    shared.mark_syncing(&account_id, generation).await;

    let result = match ensure_connection(shared, account, generation, connection).await {
        Ok(()) => {
            if let Some(gateway) = connection.gateway() {
                // Broadcast each event group as the sync produces it so mail
                // surfaces progressively instead of after the whole sync.
                let mut publish = |batch: &[DomainEvent]| shared.publish_events(batch);
                shared
                    .service
                    .sync_account_with_mode(
                        &account_id,
                        trigger.clone(),
                        mode,
                        gateway.as_ref(),
                        None,
                        &mut publish,
                    )
                    .await
            } else {
                Err(GatewayError::Unavailable(account_id.to_string()).into())
            }
        }
        Err(error) => Err(error),
    };

    match result {
        Ok(batch) => {
            let event_count = batch.len();
            ph_info!(
                events::SUPERVISOR_SYNC_COMPLETED,
                account_id = %account_id,
                trigger = trigger.as_str(),
                event_count,
                duration_ms = started.elapsed().as_millis() as u64,
                "sync completed"
            );
            shared.mark_sync_success(&account_id, generation).await;
            if let Some(reply) = reply {
                let _ = reply.send(Ok(event_count));
            }
        }
        Err(error) => {
            shared.remove_gateway(&account_id).await;
            connection.disconnect(); // tears down gateway + push stream together
            ph_error!(
                events::SUPERVISOR_SYNC_FAILED,
                account_id = %account_id,
                trigger = trigger.as_str(),
                error = %error,
                duration_ms = started.elapsed().as_millis() as u64,
                "sync failed"
            );
            if let Ok(event) = shared.service.record_sync_failure(
                &account_id,
                error.code(),
                &error.to_string(),
                trigger,
                "sync",
            ) {
                shared.publish_events(std::slice::from_ref(&event));
            }
            shared
                .mark_sync_failure(&account_id, generation, &error)
                .await;
            if let Some(reply) = reply {
                let _ = reply.send(Err(error));
            }
        }
    }
}

/// Lazily establish the gateway connection and push stream if not already
/// connected.
async fn ensure_connection(
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    generation: u64,
    connection: &mut ConnectionState,
) -> Result<(), ServiceError> {
    if connection.is_connected() {
        return Ok(());
    }
    ph_debug!(
        events::SUPERVISOR_CONNECTION_ESTABLISHING,
        account_id = %account.id,
        "establishing connection"
    );
    let conn = build_connection(account, &shared.secret_store, &shared.store).await?;
    shared.set_gateway(&account.id, conn.gateway.clone()).await;
    if conn.push_unsupported {
        shared
            .set_push_status(&account.id, generation, PushStatus::Unsupported)
            .await;
    }
    connection.set_connected(conn);
    ph_info!(
        events::SUPERVISOR_CONNECTION_ESTABLISHED,
        account_id = %account.id,
        "connection established"
    );
    Ok(())
}

async fn handle_runtime_command(
    sync_state: &Arc<SyncTriggerState>,
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    generation: u64,
    connection: &mut ConnectionState,
    command: RuntimeCommand,
) {
    match command {
        RuntimeCommand::Trigger {
            trigger,
            mode,
            reply,
        } => {
            process_sync_trigger_with_state(
                sync_state,
                shared,
                account,
                generation,
                SyncTriggerRequest::with_reply(trigger, mode, reply),
                connection,
            )
            .await;
        }
        RuntimeCommand::TriggerOnly { trigger } => {
            process_sync_trigger_with_state(
                sync_state,
                shared,
                account,
                generation,
                SyncTriggerRequest::new(trigger, SyncMode::Incremental),
                connection,
            )
            .await;
        }
    }
}

/// Automation-backfill tick: process one bounded batch of the account's
/// durable backfill job (if one is pending) and publish its events on the
/// bus, bumping the generation so connected clients refetch. Returns whether
/// the job still has work queued, so the loop shortens its next delay.
/// Without a connected gateway the batch cannot flush to the provider; the
/// store-backed job just waits for a tick after the next reconnect.
async fn handle_backfill_tick(
    shared: &Arc<SupervisorShared>,
    account_id: &AccountId,
    gateway: Option<posthaste_domain_service::SharedGateway>,
) -> bool {
    let Some(gateway) = gateway else {
        return false;
    };
    let mut publish = |batch: &[DomainEvent]| shared.publish_events(batch);
    process_backfill_batch(
        &shared.service,
        account_id,
        gateway.as_ref(),
        AUTOMATION_BACKFILL_BATCH_SIZE,
        &mut publish,
    )
    .await
}

/// Snooze scheduler tick: return every due snoozed message to the Inbox.
/// The move reuses the client path's mailbox write-through, so the provider
/// move is enqueued (flushed on the next sync) and the store invariant
/// clears the snooze row immediately.
async fn handle_snooze_tick(shared: &Arc<SupervisorShared>, account_id: &AccountId) {
    let now = SupervisorShared::monotonic_now_secs();
    match shared
        .service
        .auto_return_snoozed_messages(account_id, now)
        .await
    {
        Ok(count) if count > 0 => {
            ph_debug!(
                events::SUPERVISOR_SNOOZE_AUTO_RETURNED,
                account_id = %account_id,
                count,
                "snooze scheduler returned messages to the inbox"
            );
        }
        _ => {}
    }
}

/// Scheduled-send tick (undo-send / send-later): probe whether any held send
/// has come due and, only then, trigger a flush sync so it fires promptly
/// instead of waiting for the next poll window. Offline, the triggered
/// flush stops on the transient error and the due send fires on the next
/// connectivity window — never early, possibly late.
async fn handle_scheduled_send_tick(
    sync_state: &Arc<SyncTriggerState>,
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    generation: u64,
    connection: &mut ConnectionState,
) {
    let account_id = account.id.clone();
    match shared.service.has_due_scheduled_sends(&account_id) {
        Ok(false) => {}
        Ok(true) => {
            ph_debug!(
                events::SUPERVISOR_SCHEDULED_SEND_DUE,
                account_id = %account_id,
                "scheduled send due; triggering outbox flush sync"
            );
            process_sync_trigger_with_state(
                sync_state,
                shared,
                account,
                generation,
                SyncTriggerRequest::new(SyncTrigger::Manual, SyncMode::Incremental),
                connection,
            )
            .await;
        }
        Err(error) => {
            // A failed probe only delays the send until a later tick/poll —
            // the op itself is durable; log and keep ticking.
            ph_warn!(
                events::SUPERVISOR_SCHEDULED_SEND_PROBE_FAILED,
                account_id = %account_id,
                error = %error,
                "scheduled-send due probe failed; the held send stays queued"
            );
        }
    }
}

/// Handle one event from the resilient push stream. Returns whether a sync
/// ran (so the caller resets the poll interval).
async fn handle_push_event(
    sync_state: &Arc<SyncTriggerState>,
    shared: &Arc<SupervisorShared>,
    account: &AccountSettings,
    generation: u64,
    connection: &mut ConnectionState,
    event: PushStreamEvent,
) -> bool {
    let account_id = account.id.clone();
    match event {
        PushStreamEvent::Notification(ref notification) => {
            let remote_observation = connection
                .remote_observation()
                .unwrap_or_else(|| remote_observation_policy_for_account(account));
            ph_debug!(
                events::PUSH_NOTIFICATION_RECEIVED,
                account_id = %account_id,
                changed = ?notification.changed,
                checkpoint = notification.checkpoint.as_deref(),
                "push notification received"
            );
            if !push_notification_triggers_sync(remote_observation, notification) {
                return false;
            }
            process_sync_trigger_with_state(
                sync_state,
                shared,
                account,
                generation,
                SyncTriggerRequest::new(SyncTrigger::Push, SyncMode::Incremental),
                connection,
            )
            .await;
            true
        }
        PushStreamEvent::Connected { transport } => {
            ph_info!(
                events::PUSH_CONNECTED,
                account_id = %account_id,
                transport,
                "push connected"
            );
            shared
                .set_push_status(&account_id, generation, PushStatus::Connected)
                .await;
            // Catch-up sync on (re)connect: anything that changed during the
            // outage surfaces now instead of waiting for the next poll.
            // Routed through the coalescer, so a burst of reconnect flaps
            // collapses into a single follow-up cycle.
            process_sync_trigger_with_state(
                sync_state,
                shared,
                account,
                generation,
                SyncTriggerRequest::new(SyncTrigger::Push, SyncMode::Incremental),
                connection,
            )
            .await;
            true
        }
        PushStreamEvent::Disconnected { transport, reason } => {
            ph_warn!(
                events::PUSH_DISCONNECTED,
                account_id = %account_id,
                transport,
                reason = %reason,
                "push disconnected"
            );
            shared
                .handle_push_disconnect(&account_id, generation, &format!("{transport}: {reason}"))
                .await;
            false
        }
        PushStreamEvent::Fallback { from, to } => {
            ph_warn!(
                events::PUSH_FALLING_BACK,
                account_id = %account_id,
                from,
                to,
                "push falling back"
            );
            shared
                .handle_push_disconnect(
                    &account_id,
                    generation,
                    &format!("falling back from {from} to {to}"),
                )
                .await;
            false
        }
        PushStreamEvent::Terminal { transport, reason } => {
            // A structurally broken push transport: stop cycling
            // `Reconnecting`, mark push terminally unavailable, and rely on
            // the poll. The resilient stream has parked.
            ph_warn!(
                events::PUSH_TERMINAL,
                account_id = %account_id,
                transport,
                reason = %reason,
                "push terminally unavailable; account is poll-only"
            );
            shared
                .mark_push_terminal(&account_id, generation, transport, &reason)
                .await;
            false
        }
    }
}

fn remote_observation_policy_for_account(account: &AccountSettings) -> RemoteObservationPolicy {
    match account.driver {
        posthaste_domain_model::AccountDriver::Jmap => account
            .transport
            .provider_profile()
            .jmap()
            .remote_observation(),
        posthaste_domain_model::AccountDriver::ImapSmtp => account
            .transport
            .provider_profile()
            .imap()
            .remote_observation(),
        posthaste_domain_model::AccountDriver::Mock => RemoteObservationPolicy::disabled(),
    }
}

fn push_notification_triggers_sync(
    remote_observation: RemoteObservationPolicy,
    notification: &PushNotification,
) -> bool {
    !notification.changed.is_empty()
        || notification.checkpoint.is_some()
        || remote_observation.observes_empty_hints()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sync_trigger_state_concurrent_idle_triggers_yield_exactly_one_claim() {
        let state = SyncTriggerState::new();
        let mut tasks = Vec::new();
        for _ in 0..64 {
            let state = state.clone();
            tasks.push(tokio::spawn(async move {
                // `false` means this task won the claim.
                !state.claim_or_coalesce(SyncTrigger::Manual).await
            }));
        }
        let mut claims = 0;
        for task in tasks {
            if task.await.expect("claim task should not panic") {
                claims += 1;
            }
        }
        assert_eq!(
            claims, 1,
            "exactly one concurrent idle-window trigger may claim the cycle; the rest coalesce"
        );
    }

    #[tokio::test]
    async fn sync_trigger_state_drains_a_coalesced_trigger_on_finish() {
        let state = SyncTriggerState::new();
        state.begin_cycle().await;
        assert!(state.claim_or_coalesce(SyncTrigger::Manual).await);

        let pending = state.finish_cycle_take_pending().await;
        assert_eq!(pending, Some(SyncTrigger::Manual));
        let pending = state.finish_cycle_take_pending().await;
        assert_eq!(pending, None);
        // Idle again: the next trigger claims a fresh cycle.
        assert!(!state.claim_or_coalesce(SyncTrigger::Manual).await);
    }

    #[tokio::test]
    async fn sync_trigger_state_reset_recovers_a_stuck_active_cycle() {
        let state = SyncTriggerState::new();
        state.begin_cycle().await;
        assert!(
            state.claim_or_coalesce(SyncTrigger::Manual).await,
            "with a cycle active, a trigger coalesces"
        );
        state.reset().await;
        assert!(
            !state.claim_or_coalesce(SyncTrigger::Manual).await,
            "after reset, the next trigger claims a fresh cycle"
        );
    }

    #[test]
    fn watchdog_backoff_delay_stays_under_the_cap() {
        for attempt in 0..32 {
            assert!(watchdog_backoff_delay(attempt) <= WATCHDOG_BACKOFF_CAP);
        }
    }
}
