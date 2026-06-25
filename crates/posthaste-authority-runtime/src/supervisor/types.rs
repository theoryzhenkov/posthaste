use super::*;

pub(crate) const AUTOMATION_BACKFILL_BATCH_SIZE: usize = 10;
pub(crate) const AUTOMATION_BACKFILL_INITIAL_DELAY: Duration = Duration::from_secs(10);
pub(crate) const AUTOMATION_BACKFILL_INTERVAL: Duration = Duration::from_secs(15);
pub(crate) const CACHE_BACKGROUND_PRESSURE: f64 = 0.0;
pub(crate) const CACHE_INTERACTIVE_PRESSURE: f64 = 1.0;
pub(crate) const CACHE_STALE_RESCORE_AFTER: Duration = Duration::from_secs(6 * 60 * 60);
pub(crate) const CACHE_WORKER_INITIAL_DELAY: Duration = Duration::from_secs(5);
pub(crate) const CACHE_WORKER_INTERVAL: Duration = Duration::from_secs(2);

/// Manages per-account async runtimes: connection lifecycle, sync triggers,
/// push stream consumption, and runtime status tracking.
///
/// @spec docs/L1-sync#sync-loop
/// @spec docs/L1-api#account-crud-lifecycle
pub struct AccountSupervisor {
    pub(crate) shared: Arc<SupervisorShared>,
    pub(crate) runtimes: RwLock<HashMap<String, ManagedRuntime>>,
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
    pub(crate) known_accounts: RwLock<HashSet<String>>,
    pub(crate) account_count: AtomicUsize,
    pub(crate) cache_resources: Mutex<CacheResourceGovernor>,
    pub(crate) poll_interval: Duration,
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
    /// True while the account runtime task is inside a sync cycle.
    is_syncing: AtomicBool,
    /// A coalesced follow-up trigger that arrived while a sync was in progress.
    /// Only the most recent trigger is kept; `SyncTrigger::Manual` is the
    /// expected value for mutation-driven flushes.
    pending: Mutex<Option<SyncTrigger>>,
    /// Number of sync cycles executed by this account runtime. Used as an
    /// observability/test seam to verify that bursts of mutations do not
    /// produce one provider sync per mutation.
    sync_cycle_count: AtomicUsize,
}

impl SyncTriggerState {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            is_syncing: AtomicBool::new(false),
            pending: Mutex::new(None),
            sync_cycle_count: AtomicUsize::new(0),
        })
    }

    pub(crate) fn is_syncing(&self) -> bool {
        self.is_syncing.load(Ordering::SeqCst)
    }

    pub(crate) fn start_sync(&self) {
        self.is_syncing.store(true, Ordering::SeqCst);
    }

    pub(crate) fn finish_sync(&self) {
        self.is_syncing.store(false, Ordering::SeqCst);
    }

    pub(crate) async fn set_pending(&self, trigger: SyncTrigger) {
        let mut pending = self.pending.lock().await;
        *pending = Some(trigger);
    }

    pub(crate) async fn take_pending(&self) -> Option<SyncTrigger> {
        self.pending.lock().await.take()
    }

    pub(crate) fn increment_sync_cycle_count(&self) {
        self.sync_cycle_count.fetch_add(1, Ordering::SeqCst);
    }

    pub(crate) fn sync_cycle_count(&self) -> usize {
        self.sync_cycle_count.load(Ordering::SeqCst)
    }
}

/// A running account task and its command channel.
pub(crate) struct ManagedRuntime {
    pub(crate) command_tx: mpsc::Sender<RuntimeCommand>,
    pub(crate) handle: JoinHandle<()>,
    pub(crate) sync_state: Arc<SyncTriggerState>,
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

    pub(crate) fn set_connected(&mut self, connection: AccountConnection) {
        *self = Self::Connected(connection);
    }

    pub(crate) fn disconnect(&mut self) {
        *self = Self::Disconnected;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sync_trigger_state_starts_idle() {
        let state = SyncTriggerState::new();
        assert!(!state.is_syncing());
        assert!(state.take_pending().await.is_none());
    }

    #[tokio::test]
    async fn sync_trigger_state_tracks_pending_across_sync_cycle() {
        let state = SyncTriggerState::new();

        // Runtime begins a sync.
        state.start_sync();
        assert!(state.is_syncing());

        // A mutation arrives while the sync is running and is coalesced.
        state.set_pending(SyncTrigger::Manual).await;
        assert!(state.is_syncing());

        // Runtime finishes the first sync and immediately observes the pending
        // follow-up trigger.
        state.finish_sync();
        let pending = state.take_pending().await;
        assert_eq!(pending, Some(SyncTrigger::Manual));

        // After the follow-up is taken, the state is idle and empty again.
        assert!(!state.is_syncing());
        assert!(state.take_pending().await.is_none());
    }

    #[tokio::test]
    async fn sync_trigger_state_keeps_most_recent_pending_trigger() {
        let state = SyncTriggerState::new();
        state.start_sync();

        state.set_pending(SyncTrigger::Manual).await;
        state.set_pending(SyncTrigger::Push).await;
        state.set_pending(SyncTrigger::Manual).await;

        let pending = state.take_pending().await;
        assert_eq!(pending, Some(SyncTrigger::Manual));
    }
}
