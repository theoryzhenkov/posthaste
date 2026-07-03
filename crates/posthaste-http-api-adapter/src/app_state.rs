use super::*;

use tokio_util::sync::CancellationToken;

use crate::shutdown::{ShutdownSequence, StoreClose, SupervisorStop};

pub struct AppState {
    /// Target runtime boundary for `/v1` mail behavior.
    ///
    /// @spec docs/eph/RFC-L2-architecture-cleanup#d20
    pub runtime: RuntimeHandle,
    pub account_logo_root: PathBuf,
    /// Per-process bearer token enforced by the auth middleware when
    /// `require_auth` is on. The serialized **full-scope macaroon** (V2 base64
    /// ASCII) — opaque to the desktop shell / web client / MCP adapter, which
    /// pass it through unchanged. Always generated; inert while the flag is off.
    ///
    /// @spec docs/eph/DESIGN-L1-trust-model
    /// @spec docs/eph/DESIGN-L1-capability-tokens
    pub auth_token: String,
    /// Macaroon HMAC root key used to verify presented bearer tokens. Stage A:
    /// a token is valid iff it deserializes as a macaroon and its HMAC chain
    /// verifies against this key with no caveats to satisfy (so the full-scope
    /// token passes exactly like the former random token).
    ///
    /// @spec docs/eph/DESIGN-L1-capability-tokens
    pub macaroon_root_key: token::RootKey,
    /// Whether the `/v1` auth middleware enforces the trust model. Resolves
    /// from config (default `true`); an explicit opt-out disables it.
    ///
    /// @spec docs/eph/DESIGN-L1-trust-model
    pub require_auth: bool,
    /// Allowlisted browser origins (configured CORS origin + Tauri webview).
    ///
    /// @spec docs/eph/DESIGN-L1-trust-model
    pub origin_allowlist: Vec<String>,
    /// Allowlisted `Host` header values (loopback names + configured bind host).
    /// The mandatory DNS-rebinding defense; checked on every request when
    /// `require_auth` is on.
    ///
    /// @spec docs/eph/DESIGN-L1-trust-model
    pub host_allowlist: Vec<String>,
}

/// Handle returned by the server start path. Holds the bound address, the server
/// task, and the log guard that must survive for the process lifetime.
pub struct ServerHandle {
    pub addr: SocketAddr,
    pub join_handle: tokio::task::JoinHandle<()>,
    pub log_guard: WorkerGuard,
    /// Owns graceful runtime shutdown for the server process.
    ///
    /// @spec docs/runtime/internals/L2#runtime-shutdown-handle
    pub runtime_shutdown: RuntimeShutdownHandle,
    /// The shared cancellation token that drives graceful shutdown. Already wired
    /// into axum's `.with_graceful_shutdown`; cancelling it (via the
    /// [`ShutdownSequence`] or an embedding host) begins the HTTP drain.
    ///
    /// @spec docs/eph/RFC-L2-lifecycle-and-errors#d60
    pub shutdown_token: CancellationToken,
    /// Teardown step (b), supervisor half. `Some` for the bundled in-process
    /// server; `None` for a lean near node (no in-process supervisor).
    pub supervisor_stop: Option<Box<dyn SupervisorStop>>,
    /// Teardown step (c). `Some` for the bundled in-process server; `None` for a
    /// lean near node (no local store).
    pub store_close: Option<Box<dyn StoreClose>>,
    /// Per-process bearer token, exposed so the embedded host can inject it
    /// into the webview as `window.__POSTHASTE_TOKEN__`.
    ///
    /// @spec docs/eph/DESIGN-L1-trust-model
    pub auth_token: String,
    /// Whether the trust model is enforced. The daemon entrypoint uses this to
    /// decide whether to persist the token to `daemon.json` (an unused
    /// credential is never written to disk).
    ///
    /// @spec docs/eph/DESIGN-L1-trust-model
    pub require_auth: bool,
}

impl ServerHandle {
    /// Consume the handle into the ordered [`ShutdownSequence`] (D60). The role
    /// binaries call this after reading the fields they need up front (`addr`,
    /// `auth_token`, `require_auth`) and then `.run_until_signal()`. The log guard
    /// rides along so log output survives to the end of teardown.
    pub fn into_shutdown_sequence(self) -> ShutdownSequence {
        let mut sequence = ShutdownSequence::new(self.shutdown_token, self.join_handle)
            .with_runtime_shutdown(self.runtime_shutdown)
            .with_log_guard(self.log_guard);
        if let Some(supervisor_stop) = self.supervisor_stop {
            sequence = sequence.with_supervisor_stop(supervisor_stop);
        }
        if let Some(store_close) = self.store_close {
            sequence = sequence.with_store_close(store_close);
        }
        sequence
    }
}

/// Additional origins to allow in CORS beyond the configured default.
#[derive(Default)]
pub struct ServerConfig {
    pub extra_cors_origins: Vec<String>,
    /// Override the configured bind address (e.g. `"127.0.0.1:0"`
    /// for OS-assigned ports in the Tauri shell).
    pub bind_address_override: Option<String>,
    /// Static frontend distribution to serve for browser-localhost mode.
    pub frontend_dist: Option<PathBuf>,
}
