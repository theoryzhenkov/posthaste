use super::*;

pub(crate) const AUTOMATION_BACKFILL_BATCH_SIZE: usize = 10;
pub(crate) const AUTOMATION_BACKFILL_INITIAL_DELAY: Duration = Duration::from_secs(10);
pub(crate) const AUTOMATION_BACKFILL_INTERVAL: Duration = Duration::from_secs(15);
pub(crate) const CACHE_BACKGROUND_PRESSURE: f64 = 0.0;
pub(crate) const CACHE_INTERACTIVE_PRESSURE: f64 = 1.0;
pub(crate) const CACHE_STALE_RESCORE_AFTER: Duration = Duration::from_secs(6 * 60 * 60);
pub(crate) const CACHE_WORKER_INITIAL_DELAY: Duration = Duration::from_secs(5);
pub(crate) const CACHE_WORKER_INTERVAL: Duration = Duration::from_secs(2);
pub(crate) const OAUTH_TOKEN_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
pub(crate) const SNOOZE_INITIAL_DELAY: Duration = Duration::from_secs(30);
pub(crate) const SNOOZE_INTERVAL: Duration = Duration::from_secs(60);

/// Watchdog restart cap (RFC-L2-lifecycle §7 ruling 2 / D61): a faulting account
/// runtime is restarted at most this many times under bounded backoff; the
/// failure that would require one more restart halts it with a truthful status.
pub(crate) const WATCHDOG_MAX_RESTARTS: u32 = 3;
/// A single incarnation that stays healthy for at least this long resets the
/// restart budget — a fault after a sustained-healthy run is a fresh incident,
/// not part of a restart storm. **Review** (named for owner review per D61).
pub(crate) const WATCHDOG_HEALTHY_RESET_AFTER: Duration = Duration::from_secs(60);
/// Deadline for a single cooperative per-account stop (`stop_account`) before the
/// escalation aborts the incarnation + its watchdog. Mirrors the M20 supervisor
/// phase budget so a per-account stop cannot outlast the whole-supervisor one.
pub(crate) const PER_ACCOUNT_STOP_DEADLINE: Duration = Duration::from_secs(3);

// ---------------------------------------------------------------------------
// Scheduling governor (RFC-L2-provider-reliability D98 / Sc1 / R4, ruling O7).
// A DISTINCT governor from `CacheResourceGovernor` (which throttles cache
// fetches only — the sync path never consulted it, R4): this one bounds the
// provider-sync fan-out across ALL accounts and decorrelates the boot herd.
// Both values are **Review** (picked sane, not measured; named for owner
// review, matching `WATCHDOG_HEALTHY_RESET_AFTER`'s posture).
// ---------------------------------------------------------------------------

/// Global ceiling on concurrent provider sync cycles across every account
/// (D98(b) / R4). Without it, N accounts syncing at boot open N simultaneous
/// provider connections; with it, the sync path holds one of these slots for
/// the duration of a cycle, so at most this many run at once and the rest queue.
pub(crate) const GLOBAL_CONCURRENT_SYNC_LIMIT: usize = 8;

/// Upper bound on the random startup-sync splay (D98(a) / Sc1). Each account's
/// *initial* `Startup` sync is delayed by a uniform draw in
/// `[0, STARTUP_SYNC_SPLAY_MAX)` so N accounts started in a tight boot loop do
/// not all fire their first provider sync at the same instant (the boot storm).
/// Only the boot path splays; interactive create/patch/enable start immediately.
pub(crate) const STARTUP_SYNC_SPLAY_MAX: Duration = Duration::from_secs(5);

/// The supervisor's scheduling governor: the global concurrent-sync limiter plus
/// the startup-splay ceiling (D98). Held on [`SupervisorShared`]; injectable so a
/// test can pin a tiny cap / zero splay. Explicitly NOT the
/// [`CacheResourceGovernor`](posthaste_domain_service::CacheResourceGovernor),
/// which governs cache fetches only (ruling O7).
pub(crate) struct SyncGovernor {
    /// One permit per allowed concurrent provider sync (see
    /// [`GLOBAL_CONCURRENT_SYNC_LIMIT`]). `Arc` so a cycle can hold an owned
    /// permit across its `.await` points.
    pub(crate) slots: Arc<Semaphore>,
    /// See [`STARTUP_SYNC_SPLAY_MAX`]. Zero disables the splay entirely.
    pub(crate) startup_splay_max: Duration,
}

impl SyncGovernor {
    /// Production governor: the ratified global cap + startup-splay ceiling.
    pub(crate) fn production() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(GLOBAL_CONCURRENT_SYNC_LIMIT)),
            startup_splay_max: STARTUP_SYNC_SPLAY_MAX,
        }
    }

    /// Test governor with an explicit cap and splay (unit tests only).
    #[cfg(test)]
    pub(crate) fn for_test(concurrent_sync_limit: usize, startup_splay_max: Duration) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(concurrent_sync_limit)),
            startup_splay_max,
        }
    }
}

// ---------------------------------------------------------------------------
// Select-loop arm budgets (RFC-L2-lifecycle D66 / M26). Every inline await
// inside an account runtime's `select!` arm (`runtime.rs`) is wrapped in
// `tokio::time::timeout` at its call site, so a hung provider/oauth/store
// call cannot wedge the whole account loop undetectably (audit row 5 / N17).
//
// These are a BACKSTOP, not the primary control. Individual provider calls
// already carry their own tighter "envelope" deadline: `IMAP_OP_TIMEOUT_MS`
// (60s, `posthaste-imap/src/timeout.rs`) bounds a single IMAP round trip;
// `OAUTH_HTTP_TOTAL_TIMEOUT` (30s, `oauth/service.rs`) bounds a single IdP
// HTTP call. Under normal operation a genuinely hung call trips its own
// envelope deadline well before the arm budget below is reached — the arm
// budget only fires when either (a) many bounded-but-slow envelope calls
// chain inside one cycle (a large mailbox sync issuing hundreds of sub-60s
// round trips) or (b) some call path the envelope layer does not cover hangs
// outright. Each budget here is set comfortably above its arm's worst
// normal-case envelope total — belt over braces (principle VI). All are
// **Review** (picked sane, not measured; named for owner review per D66,
// matching `WATCHDOG_HEALTHY_RESET_AFTER`'s review posture).
// ---------------------------------------------------------------------------

/// Budget for a full sync cycle: the poll tick, a manual/API-triggered
/// command, and a push-notification-driven sync all route through
/// `process_sync_trigger_with_state`, which may loop to drain a coalesced
/// follow-up trigger. The longest budget — a real sync can issue many IMAP
/// round trips (each individually capped at `IMAP_OP_TIMEOUT_MS`), so this
/// must clear a large-but-progressing mailbox, not just one op.
pub(crate) const ARM_BUDGET_SYNC: Duration = Duration::from_secs(300);
/// Budget for one automation-backfill batch
/// (`AUTOMATION_BACKFILL_BATCH_SIZE` = 10 provider round trips per tick).
pub(crate) const ARM_BUDGET_BACKFILL: Duration = Duration::from_secs(120);
/// Budget for one cache-maintenance batch (a bounded fetch/rescore batch
/// under the cache resource governor's lease).
///
/// This arm budget is the *last-resort backstop*, not the working bound: the
/// body-cache batch carries its own deadline
/// ([`posthaste_domain_model::BODY_CACHE_BATCH_BUDGET`], checked per candidate
/// with the in-flight fetch bounded to the remaining budget), so under a slow
/// or hung provider the batch returns with partial work — and records governor
/// feedback/backoff — well before this timeout can drop it. If this arm budget
/// ever does fire (e.g. a wedged local store call the batch deadline does not
/// cover), the runtime additionally records a cancelled slice on the governor
/// so the cache tick backs off instead of re-wedging every 2 s.
pub(crate) const ARM_BUDGET_CACHE: Duration = Duration::from_secs(120);

// The batch deadline must leave the arm budget as a comfortably-later
// backstop; if either constant is retuned, keep the batch's worst case
// (deadline check + one remaining-budget-bounded fetch + local store writes)
// well inside the arm budget.
const _: () = assert!(
    BODY_CACHE_BATCH_BUDGET.as_millis() * 2 <= ARM_BUDGET_CACHE.as_millis(),
    "BODY_CACHE_BATCH_BUDGET must stay well under ARM_BUDGET_CACHE (the arm is the backstop)"
);
/// Budget for one snooze-scheduler tick. Local-store-only — it reads
/// `message_snooze` and enqueues a mailbox-move outbox op rather than
/// sending one inline — so this is the tightest budget: a backstop against a
/// wedged store lock, not a provider network call.
pub(crate) const ARM_BUDGET_SNOOZE: Duration = Duration::from_secs(30);
/// Budget for one OAuth-refresh tick: `resolve_secret` (bounded by
/// `OAUTH_HTTP_TOTAL_TIMEOUT` when it round-trips the IdP) plus, on a token
/// rotation, a full `ensure_connection` gateway rebuild.
pub(crate) const ARM_BUDGET_OAUTH_REFRESH: Duration = Duration::from_secs(90);

/// Manages per-account async runtimes: connection lifecycle, sync triggers,
/// push stream consumption, and runtime status tracking.
///
/// @spec docs/L1-sync#sync-loop
/// @spec docs/L1-api#account-crud-lifecycle
pub struct AccountSupervisor {
    pub(crate) shared: Arc<SupervisorShared>,
    pub(crate) runtimes: RwLock<HashMap<String, ManagedRuntime>>,
    /// Supervisor-level cancellation token. Each account runtime's token is a
    /// child of this one, so `stop_all` cancels every account cooperatively with
    /// a single `cancel()` (D61). Per-account `stop_account` cancels only that
    /// account's child, leaving siblings running.
    pub(crate) root_cancel: CancellationToken,
}

/// Shared state across all account runtimes: services, event bus, and runtime overviews.
pub(crate) struct SupervisorShared {
    pub(crate) service: Arc<MailService>,
    pub(crate) store: Arc<dyn MailStore>,
    pub(crate) secret_store: Arc<dyn SecretStore>,
    pub(crate) event_sender: broadcast::Sender<DomainEvent>,
    pub(crate) gateways: RwLock<HashMap<String, SharedGateway>>,
    pub(crate) runtime_overviews: RwLock<HashMap<String, AccountRuntimeOverview>>,
    pub(crate) runtime_generations: RwLock<HashMap<String, RuntimeGeneration>>,
    pub(crate) sync_cycle_generations: RwLock<HashMap<String, SyncCycleGeneration>>,
    pub(crate) known_accounts: RwLock<HashSet<String>>,
    pub(crate) account_count: AtomicUsize,
    pub(crate) cache_resources: Mutex<CacheResourceGovernor>,
    /// Scheduling governor (D98 / R4 / O7): the global concurrent-sync limiter
    /// and the startup-splay ceiling. Distinct from `cache_resources` above.
    pub(crate) sync_governor: SyncGovernor,
    pub(crate) poll_interval: Duration,
    /// Per-secret-ref OAuth refresh single-flight (the minimal M37 piece
    /// pulled into M34): concurrent resolvers of the same secret — IMAP
    /// session reconnects, SMTP sends, the proactive refresh tick — serialize
    /// here, and each flight re-reads the stored token set under the lock, so
    /// two racing refreshes can no longer both exchange and then clobber each
    /// other's rotated refresh token in the secret store (audit A1).
    pub(crate) oauth_refresh_flights: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

/// Monotonic identity for a spawned account runtime.
///
/// Async tasks capture the generation they were spawned with. Any delayed
/// status/progress/push write from an older generation is ignored, preventing a
/// stopped/restarted account task from overwriting the current runtime state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeGeneration(u64);

impl RuntimeGeneration {
    pub(crate) const INITIAL: Self = Self(0);

    pub(crate) fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// Monotonic identity for a single sync cycle within one account incarnation
/// (RFC-L2-lifecycle N5 + the M26 flag / M27 sub-unit (d)).
///
/// [`RuntimeGeneration`] only changes when the account's whole incarnation is
/// replaced (a watchdog restart) — it does NOT change between sync cycles
/// within the same incarnation. That leaves a gap: when a select!-loop arm's
/// `tokio::time::timeout` (D66) abandons a hung sync cycle, the cycle's
/// progress-forwarder task (`sync_flow::sync_progress_reporter`) is a
/// separately-spawned task, not owned by the timed-out future — dropping the
/// future does not cancel it. Without a finer-grained guard, a progress write
/// already in flight from that abandoned cycle still carries a *matching*
/// `RuntimeGeneration` and can land after `mark_arm_timeout` sets `Degraded`,
/// flipping status back to `Syncing` — the M26 flap this type closes.
///
/// [`SupervisorShared::next_sync_cycle_generation`] mints one of these at the
/// start of every sync cycle and again to invalidate the current one when an
/// arm abandons a cycle (`record_arm_timeout`); progress writes carry the
/// value minted for their own cycle, and are rejected once it no longer
/// matches the account's current cycle token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SyncCycleGeneration(u64);

impl SyncCycleGeneration {
    pub(crate) const INITIAL: Self = Self(0);

    pub(crate) fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// Capacity of the bounded channel [`sync_flow::sync_progress_reporter`] uses
/// to forward progress updates to its single per-cycle writer task. Progress
/// values are display-only and monotonically superseded by the next one, so a
/// full channel drops the newest update via `try_send` rather than blocking
/// the (synchronous) reporter callback — a small capacity just smooths a
/// brief burst faster than the writer can drain it. **Review**.
pub(crate) const SYNC_PROGRESS_CHANNEL_CAPACITY: usize = 8;

/// Coordinates fire-and-forget sync triggers between the supervisor and the
/// per-account runtime task. When the runtime is already executing a sync
/// cycle, additional `TriggerOnly` requests are coalesced into a single
/// pending trigger rather than enqueueing a full sync for each request.
///
/// This prevents a burst of local mutations (e.g. rapid flag toggles) from
/// producing one provider sync per mutation. One sync cycle drains all pending
/// local-first operations, so coalescing preserves correctness while avoiding
/// serial sync storms.
pub(crate) struct SyncTriggerState {
    /// The `active` claim flag and coalesced `pending` trigger guarded together
    /// so the trigger source and the runtime drain loop make one atomic
    /// decision. Splitting these across an `AtomicBool` + separate `Mutex`
    /// allowed a lost-wakeup race: a trigger observed the cycle running, then
    /// the drain loop cleared the flag and took an (empty) pending before the
    /// trigger stored its own — stranding it so its sync never ran and open
    /// views went stale until the next poll.
    inner: Mutex<SyncCoalesceState>,
    /// Number of sync cycles executed by this account runtime. Used as an
    /// observability/test seam to verify that bursts of mutations do not
    /// produce one provider sync per mutation.
    sync_cycle_count: AtomicUsize,
}

#[derive(Default)]
struct SyncCoalesceState {
    /// True from the instant a cycle is ADMITTED until it — and any coalesced
    /// follow-up drained from `pending` — fully finishes. Admission is either:
    /// (a) the first fire-and-forget trigger winning the atomic claim in
    /// [`SyncTriggerState::claim_or_coalesce`], which sets this flag under the
    /// lock *before* the trigger is enqueued onto the runtime's command
    /// channel; or (b) a directly-driven cycle (poll / push / manual / startup)
    /// calling [`SyncTriggerState::begin_cycle`].
    ///
    /// Case (a) is the D99 fix. The old design only set the flag at
    /// `begin_cycle`, i.e. when the runtime task *dequeued* the first trigger —
    /// so between a trigger's enqueue and that dequeue the coalescer still read
    /// idle, and N concurrent idle-window triggers each enqueued their own
    /// redundant cycle (the P5 flake). Setting the flag at claim time makes the
    /// idle→active transition a single atomic compare-and-set: exactly one
    /// concurrent trigger claims the cycle, every other coalesces into
    /// `pending`, and the coalesced count is now an invariant, not a race.
    active: bool,
    /// A coalesced follow-up trigger that arrived while a cycle was admitted.
    /// Only the most recent trigger is kept; `SyncTrigger::Manual` is the
    /// expected value for mutation-driven flushes.
    pending: Option<SyncTrigger>,
}

impl SyncTriggerState {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(SyncCoalesceState::default()),
            sync_cycle_count: AtomicUsize::new(0),
        })
    }

    /// Trigger-source entry point (the supervisor manager) and the D99 atomic
    /// claim. Under one lock: if a cycle is already admitted (`active`), coalesce
    /// this trigger into the pending follow-up and return `true` (the caller must
    /// not enqueue a new cycle). Otherwise atomically CLAIM the cycle — set
    /// `active` here, before the caller enqueues — and return `false` (the caller
    /// enqueues exactly one cycle, whose [`begin_cycle`] finds `active` already
    /// set).
    ///
    /// Setting `active` at claim time (not at `begin_cycle`) is the fix for the
    /// P5 idle-boundary race: N concurrent idle-window triggers now serialize on
    /// this lock, the first flips idle→active and claims, and every other sees
    /// `active` and coalesces — so a burst can never enqueue more than one cycle
    /// plus one pending follow-up.
    ///
    /// The check and the store happen under one lock, so a trigger can never be
    /// stranded between the drain loop clearing `active` and taking `pending`.
    pub(crate) async fn claim_or_coalesce(&self, trigger: SyncTrigger) -> bool {
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
    /// Idempotent on `active` for the claim path (the claim already set it); the
    /// atomic set for the directly-driven paths (poll / push / manual / startup)
    /// that do not go through [`claim_or_coalesce`].
    pub(crate) async fn begin_cycle(&self) {
        self.inner.lock().await.active = true;
        self.sync_cycle_count.fetch_add(1, Ordering::SeqCst);
    }

    /// Finish the current cycle. If a trigger was coalesced while it ran, take it
    /// (keeping `active` set so the caller runs a follow-up cycle); otherwise
    /// clear `active` and return `None`. The take-or-clear is atomic with
    /// [`claim_or_coalesce`], which is what closes the lost-wakeup race.
    pub(crate) async fn finish_cycle_take_pending(&self) -> Option<SyncTrigger> {
        let mut inner = self.inner.lock().await;
        match inner.pending.take() {
            Some(trigger) => Some(trigger),
            None => {
                inner.active = false;
                None
            }
        }
    }

    pub(crate) fn sync_cycle_count(&self) -> usize {
        self.sync_cycle_count.load(Ordering::SeqCst)
    }

    /// Clear the coalescing state for a fresh incarnation after a watchdog
    /// restart. Without this a panic mid-cycle would leave `active == true`, so
    /// the restarted runtime would coalesce every trigger forever instead of
    /// enqueueing a cycle. The cycle counter is intentionally preserved as an
    /// account-lifetime observability signal.
    pub(crate) async fn reset(&self) {
        let mut inner = self.inner.lock().await;
        inner.active = false;
        inner.pending = None;
    }

    #[cfg(test)]
    pub(crate) async fn is_syncing(&self) -> bool {
        self.inner.lock().await.active
    }
}

/// A supervised account: the watchdog task plus the swappable handles the
/// watchdog rewires each time it restarts the underlying incarnation.
///
/// The command channel's receiver is consumed by each incarnation task, so a
/// restart must mint a fresh channel; `command_tx` therefore lives behind a slot
/// the watchdog swaps (callers always reach the live incarnation, or get `None`
/// during the restart gap). `incarnation_abort` targets the *current* incarnation
/// for the post-deadline escalation (NOT the primary stop path). `monitor` is the
/// supervisor-owned watchdog that observes the incarnation JoinHandle (surfacing
/// panics) and restarts under the ratified policy.
pub(crate) struct ManagedRuntime {
    pub(crate) command_tx: Arc<StdMutex<Option<mpsc::Sender<RuntimeCommand>>>>,
    pub(crate) sync_state: Arc<SyncTriggerState>,
    pub(crate) cancel: CancellationToken,
    pub(crate) incarnation_abort: Arc<StdMutex<Option<AbortHandle>>>,
    pub(crate) monitor: JoinHandle<()>,
}

/// Commands sent to a running account runtime via the mpsc channel.
pub(crate) enum RuntimeCommand {
    Trigger {
        trigger: SyncTrigger,
        mode: SyncMode,
        reply: oneshot::Sender<Result<usize, ServiceError>>,
    },
    TriggerOnly {
        trigger: SyncTrigger,
    },
    CacheMaintenance {
        interactive_pressure: f64,
        operation_id: Option<String>,
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
pub(crate) struct AccountConnection {
    pub(crate) gateway: SharedGateway,
    pub(crate) push_events: Option<PushEventStream>,
    pub(crate) remote_observation: RemoteObservationPolicy,
    /// Secret resolver used to refresh OAuth tokens before they expire. Present
    /// for all live connections so the runtime can proactively rebuild JMAP
    /// gateways whose client bakes auth in at construction.
    pub(crate) secret_resolver: Arc<dyn SecretResolver>,
}

/// Local runtime connection state. Keeps gateway and push stream lifetimes coupled.
#[derive(Default)]
pub(crate) enum AccountRuntimeConnectionState {
    #[default]
    Disconnected,
    Connected(AccountConnection),
}

impl AccountRuntimeConnectionState {
    pub(crate) fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }

    pub(crate) fn gateway(&self) -> Option<SharedGateway> {
        match self {
            Self::Connected(connection) => Some(connection.gateway.clone()),
            Self::Disconnected => None,
        }
    }

    pub(crate) fn remote_observation(&self) -> Option<RemoteObservationPolicy> {
        match self {
            Self::Connected(connection) => Some(connection.remote_observation),
            Self::Disconnected => None,
        }
    }

    pub(crate) fn push_events_mut(&mut self) -> Option<&mut PushEventStream> {
        match self {
            Self::Connected(connection) => connection.push_events.as_mut(),
            Self::Disconnected => None,
        }
    }

    pub(crate) fn secret_resolver(&self) -> Option<Arc<dyn SecretResolver>> {
        match self {
            Self::Connected(connection) => Some(Arc::clone(&connection.secret_resolver)),
            Self::Disconnected => None,
        }
    }

    pub(crate) fn set_connected(&mut self, connection: AccountConnection) {
        *self = Self::Connected(connection);
    }

    pub(crate) fn disconnect(&mut self) {
        *self = Self::Disconnected;
    }
}

/// Tracks the last resolved OAuth access token for an account runtime so the
/// loop can detect when the secret store has a fresher token than the gateway.
///
/// JMAP gateways hold a jmap_client::Client whose bearer token is fixed at
/// construction. When `resolve_secret()` returns a new token, the runtime tears
/// the connection down so the next operation rebuilds with the fresh token.
pub(crate) struct OAuthRefreshState {
    enabled: bool,
    last_secret: Option<String>,
    interval: Option<tokio::time::Interval>,
}

impl OAuthRefreshState {
    pub(crate) fn new(account: &AccountSettings) -> Self {
        let enabled = account.transport.auth == ProviderAuthKind::OAuth2;
        Self {
            enabled,
            last_secret: None,
            interval: None,
        }
    }

    pub(crate) fn interval(&mut self) -> Option<tokio::time::Interval> {
        if !self.enabled {
            return None;
        }
        if self.interval.is_none() {
            let mut interval = tokio::time::interval(OAUTH_TOKEN_REFRESH_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            self.interval = Some(interval);
        }
        self.interval.take()
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn last_secret(&self) -> Option<&str> {
        self.last_secret.as_deref()
    }

    pub(crate) fn set_last_secret(&mut self, secret: String) {
        self.last_secret = Some(secret);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sync_trigger_state_idle_trigger_claims_and_is_not_coalesced() {
        let state = SyncTriggerState::new();
        assert!(!state.is_syncing().await);
        // Idle: the first trigger CLAIMS the cycle (returns false → the caller
        // enqueues) and atomically marks the state active, before any
        // `begin_cycle`.
        assert!(!state.claim_or_coalesce(SyncTrigger::Manual).await);
        assert!(
            state.is_syncing().await,
            "the claim marks the state active immediately, before begin_cycle"
        );
        // A second trigger now coalesces rather than claiming its own cycle.
        assert!(state.claim_or_coalesce(SyncTrigger::Manual).await);
    }

    #[tokio::test]
    async fn sync_trigger_state_concurrent_idle_triggers_yield_exactly_one_claim() {
        // The headline D99 invariant: N triggers racing the idle→active boundary
        // produce exactly ONE claim (one enqueued cycle); every other coalesces.
        // Before the atomic claim this was probabilistic (each could observe
        // idle and enqueue its own cycle) — the root of the P5 flake.
        let state = SyncTriggerState::new();
        let mut tasks = Vec::new();
        for _ in 0..64 {
            let state = state.clone();
            tasks.push(tokio::spawn(async move {
                // `false` means this task WON the claim.
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
    async fn sync_trigger_state_reset_recovers_a_stuck_active_after_an_arm_timeout() {
        // Review finding #1: a sync cycle whose future is DROPPED mid-flight
        // (arm-budget timeout on a hung provider) leaves `active` stuck true, so
        // every later trigger coalesces without ever enqueuing a cycle. reset()
        // — called from record_arm_timeout — must recover: the next trigger
        // claims a fresh cycle instead of coalescing into a dead one.
        let state = SyncTriggerState::new();
        state.begin_cycle().await; // cycle starts...
        // ...its future is dropped before finish_cycle (the timeout). A trigger
        // arriving now would coalesce (active is stuck).
        assert!(
            state.claim_or_coalesce(SyncTrigger::Manual).await,
            "with active stuck, a trigger coalesces (the bug)"
        );
        state.reset().await; // record_arm_timeout's recovery
        assert!(
            !state.claim_or_coalesce(SyncTrigger::Manual).await,
            "after reset, the next trigger claims a fresh cycle (the fix)"
        );
    }

    #[tokio::test]
    async fn sync_trigger_state_drains_a_coalesced_trigger_on_finish() {
        let state = SyncTriggerState::new();

        // Runtime begins a sync.
        state.begin_cycle().await;
        assert!(state.is_syncing().await);

        // A mutation arrives while the sync is running and is coalesced.
        assert!(state.claim_or_coalesce(SyncTrigger::Manual).await);
        assert!(state.is_syncing().await);

        // Finishing the cycle takes the coalesced follow-up (and keeps syncing
        // set so the caller runs it) — the trigger is never stranded.
        let pending = state.finish_cycle_take_pending().await;
        assert_eq!(pending, Some(SyncTrigger::Manual));

        // Finishing again with nothing pending clears syncing and returns None.
        let pending = state.finish_cycle_take_pending().await;
        assert_eq!(pending, None);
        assert!(!state.is_syncing().await);
    }

    #[tokio::test]
    async fn sync_trigger_state_keeps_most_recent_pending_trigger() {
        let state = SyncTriggerState::new();
        state.begin_cycle().await;

        assert!(state.claim_or_coalesce(SyncTrigger::Manual).await);
        assert!(state.claim_or_coalesce(SyncTrigger::Push).await);
        assert!(state.claim_or_coalesce(SyncTrigger::Manual).await);

        let pending = state.finish_cycle_take_pending().await;
        assert_eq!(pending, Some(SyncTrigger::Manual));
    }

    #[tokio::test]
    async fn sync_trigger_state_finishing_idle_window_does_not_strand_a_later_trigger() {
        // Regression for the lost-wakeup race: a cycle finishes with nothing
        // pending (clearing syncing), and only afterwards a trigger arrives.
        // Because the trigger now observes `syncing == false` under the same
        // lock the drain used, it is NOT coalesced — the caller enqueues a fresh
        // cycle instead of stranding it in `pending`.
        let state = SyncTriggerState::new();
        state.begin_cycle().await;

        // Drain finds nothing pending and clears syncing.
        assert_eq!(state.finish_cycle_take_pending().await, None);
        assert!(!state.is_syncing().await);

        // A trigger that arrives now is not swallowed into pending; it claims a
        // fresh cycle (returns false → the caller enqueues one).
        assert!(!state.claim_or_coalesce(SyncTrigger::Manual).await);
    }
}
