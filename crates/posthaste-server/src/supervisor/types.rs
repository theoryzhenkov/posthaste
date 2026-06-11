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
    pub(crate) cache_resources: Mutex<CacheResourceGovernor>,
    pub(crate) poll_interval: Duration,
}

/// A running account task and its command channel.
pub(crate) struct ManagedRuntime {
    pub(crate) command_tx: mpsc::Sender<RuntimeCommand>,
    pub(crate) handle: JoinHandle<()>,
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
