//! Runtime assembly + build config (D29 split from `build.rs`): the
//! transport-free build inputs, the remote/colocated build entry points, and the
//! shared [`assemble_runtime`] that composes a near node over a backend link.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use posthaste_contract_core::{RuntimeLifecycle, RuntimeStatus, RuntimeStoreStatus};
use posthaste_domain_service::{DomainEvent, SecretStore};
use posthaste_link_contract::{BackendApi, BackendLink};
use tokio::sync::broadcast;

use crate::handle::{RuntimeCoreState, RuntimeHandle};
use crate::near_node::RuntimeBackendOutbox;
use crate::read::ReadCache;
use crate::secret::SystemSecretStore;
use crate::sessions::SessionRegistry;
use crate::shutdown::RuntimeShutdownHandle;
use crate::transport::RemoteBackend;
use crate::views::ViewRegistry;

const DEFAULT_EVENT_CHANNEL_CAPACITY: usize = 512;

/// Transport-free build inputs for the local authority runtime.
///
/// Roots are resolved by the host before construction so the runtime owns mail
/// authority state without depending on renderer storage.
///
/// spec: docs/runtime/internals/L2#runtime-builder-transport-free
/// spec: docs/runtime/internals/L1#runtime-owned-roots
pub struct RuntimeBuildConfig {
    pub config_root: PathBuf,
    pub state_root: PathBuf,
    pub cache_root: PathBuf,
    pub bootstrap_path: Option<PathBuf>,
    pub secret_store: Option<Arc<dyn SecretStore>>,
    pub event_channel_capacity: usize,
    pub poll_interval: Duration,
    /// Which transport carries the runtime↔backend link ([replication backend-link L2 §6](../replication/backend-link/L2.md)).
    /// Chosen from configuration, not at build time; the default is in-process
    /// co-located (assertion `transport-selected-by-config`).
    pub backend_transport: BackendTransportConfig,
    /// A decorator over the config-selected link transport. When set, the
    /// builder hands it the real (in-process or remote) [`BackendApi`] and uses
    /// what it returns. A host/test seam for *composing over* the transport
    /// (e.g. gating the up-channel to exercise the near-node outbox) without
    /// replacing the full backend surface — the decorator delegates everything
    /// it does not intercept to the inner transport. `None` in normal builds.
    pub backend_transport_override: Option<BackendTransportDecorator>,
}

/// A decorator over the config-selected link transport (see
/// [`RuntimeBuildConfig::backend_transport_override`]): receives the
/// real [`BackendApi`] and returns a wrapping one. Composes, so it need not
/// re-implement the whole surface — only the methods it intercepts.
pub type BackendTransportDecorator =
    Box<dyn FnOnce(Arc<dyn BackendApi>) -> Arc<dyn BackendApi> + Send>;

/// The runtime↔backend link transport, selected by configuration.
///
/// `InProcess` (default) is the co-located far node — zero serialization, byte
/// for byte the pre-link behavior. `Remote` points the link at a backend that
/// serves the link wire (POST up + SSE down) elsewhere; switching is a config
/// change, not a rebuild ([replication backend-link L2 §6](../replication/backend-link/L2.md)).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BackendTransportConfig {
    #[default]
    InProcess,
    Remote {
        base_url: String,
        /// Bearer token presented to the backend's authenticated `link_router`
        /// (`LinkAuth::PerRuntime`). `None` for an unauthenticated link.
        token: Option<String>,
    },
}

impl RuntimeBuildConfig {
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

    /// Decorate the config-selected link transport (see
    /// [`backend_transport_override`](Self::backend_transport_override)). The
    /// closure receives the real transport and returns the one the link uses.
    pub fn with_backend_transport_override(
        mut self,
        decorator: impl FnOnce(Arc<dyn BackendApi>) -> Arc<dyn BackendApi> + Send + 'static,
    ) -> Self {
        self.backend_transport_override = Some(Box::new(decorator));
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
/// spec: docs/runtime/internals/L1#runtime-handle-transport-neutral
pub struct RemoteRuntimeBuild {
    pub handle: RuntimeHandle,
    pub shutdown: RuntimeShutdownHandle,
    pub runtime_status: RuntimeStatus,
    /// The runtime's own secret store, for its `/v1` client auth.
    pub secret_store: Arc<dyn SecretStore>,
}

/// Build a backend-less runtime near node over a remote backend link. Requires
/// [`BackendTransportConfig::Remote`] (a near node has no in-process backend to
/// fall back to). Must run within a Tokio runtime: it spawns the down-channel
/// bridge that keeps the read cache + views live from the backend's assertions.
pub fn build_remote_runtime(
    config: RuntimeBuildConfig,
) -> Result<RemoteRuntimeBuild, crate::shutdown::RuntimeBuildError> {
    if config.event_channel_capacity == 0 {
        return Err(crate::shutdown::RuntimeBuildError::InvalidConfig(
            "event_channel_capacity must be greater than zero".to_string(),
        ));
    }
    let RuntimeBuildConfig {
        secret_store,
        event_channel_capacity,
        backend_transport,
        backend_transport_override,
        ..
    } = config;

    let (base_url, token) = match backend_transport {
        BackendTransportConfig::Remote { base_url, token } => (base_url, token),
        BackendTransportConfig::InProcess => {
            return Err(crate::shutdown::RuntimeBuildError::InvalidConfig(
                "a remote runtime requires a remote backend transport".to_string(),
            ));
        }
    };

    let secret_store = secret_store.unwrap_or_else(|| Arc::new(SystemSecretStore));
    let (event_sender, _) = broadcast::channel(event_channel_capacity);

    // The link transport is the remote backend (optionally decorated by a test
    // seam); there is no in-process far node to fall back to.
    let base: Arc<dyn BackendApi> = Arc::new(RemoteBackend::with_token(base_url, token));
    let transport = match backend_transport_override {
        Some(decorate) => decorate(base),
        None => base,
    };
    let backend_link = BackendLink::new(transport);
    let reads = Arc::new(ReadCache::retaining(backend_link.transport().clone()));

    // No local store; the live account count comes through the link on the
    // `runtime_status` read.
    let runtime_status = RuntimeStatus {
        lifecycle: RuntimeLifecycle::Ready,
        store: RuntimeStoreStatus {
            config_loaded: true,
            state_store_open: false,
            cache_root_ready: false,
        },
        account_count: 0,
    };

    // A near node drives its cache + views from the backend down-channel (it has
    // no local event bus of its own).
    let composed = assemble_runtime(RuntimeAssembly {
        backend_link,
        reads,
        event_sender,
        startup_status: runtime_status.clone(),
        drive_down_channel: true,
    });

    Ok(RemoteRuntimeBuild {
        handle: composed.handle,
        shutdown: composed.shutdown,
        runtime_status,
        secret_store,
    })
}

/// Inputs for [`assemble_runtime`]: a link to the backend plus the read cache
/// over it. The far-node crate builds these around an in-process `LocalBackend`;
/// [`build_remote_runtime`] builds them around a [`RemoteBackend`].
pub struct RuntimeAssembly {
    /// The runtime↔backend link over its (config-selected) transport.
    pub backend_link: BackendLink,
    /// The read-through cache over the same transport the link uses.
    pub reads: Arc<ReadCache>,
    /// The runtime's domain-event bus. In-process this is the backend's bus; a
    /// remote near node owns its own and the down-channel republishes onto it.
    pub event_sender: broadcast::Sender<DomainEvent>,
    /// The startup status snapshot the handle reports until live reads layer on.
    pub startup_status: RuntimeStatus,
    /// Spawn the backend down-channel bridge (a remote near node: evict on
    /// assertions and republish so views recompute). In-process the runtime
    /// shares the backend's bus, so no bridge is needed.
    pub drive_down_channel: bool,
}

/// The handle + shutdown produced by [`assemble_runtime`].
pub struct ComposedRuntime {
    pub handle: RuntimeHandle,
    pub shutdown: RuntimeShutdownHandle,
}

/// Assemble a runtime near node over a backend link: the outbox, view/session
/// registries, and the handle. The far-node crate calls this to compose an
/// in-process runtime over a `LocalBackend`; [`build_remote_runtime`] calls it
/// over a [`RemoteBackend`]. Must run within a Tokio runtime when
/// `drive_down_channel` is set (it spawns the down-channel bridge).
pub fn assemble_runtime(assembly: RuntimeAssembly) -> ComposedRuntime {
    let RuntimeAssembly {
        backend_link,
        reads,
        event_sender,
        startup_status,
        drive_down_channel,
    } = assembly;

    let stopped = Arc::new(AtomicBool::new(false));
    // Remote backend (`drive_down_channel`): retirement is absorption-gated on
    // the down-channel base assertion, not the receipt. Co-located: retire on
    // receipt (`colocated-unchanged`). See [`RuntimeBackendOutbox`].
    let outbox = Arc::new(RuntimeBackendOutbox::new(drive_down_channel));
    if drive_down_channel {
        tokio::spawn(crate::read::run_backend_down_channel(
            backend_link.clone(),
            reads.clone(),
            event_sender.clone(),
            outbox.clone(),
        ));
    }
    let views = Arc::new(ViewRegistry::new(
        event_sender.clone(),
        outbox.clone(),
        reads.clone(),
    ));
    let sessions = Arc::new(SessionRegistry::new(views.clone(), event_sender.clone()));
    let core = Arc::new(RuntimeCoreState {
        backend_link,
        outbox,
        reads,
        event_sender,
        views,
        sessions,
        startup_status,
        stopped: stopped.clone(),
    });

    ComposedRuntime {
        handle: RuntimeHandle { core },
        shutdown: RuntimeShutdownHandle { stopped },
    }
}
