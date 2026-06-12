use super::*;

pub struct AppState {
    pub service: Arc<MailService>,
    pub store: Arc<dyn MailStore>,
    pub secret_store: Arc<dyn SecretStore>,
    pub supervisor: Arc<AccountSupervisor>,
    pub event_sender: broadcast::Sender<DomainEvent>,
    pub account_logo_root: PathBuf,
    pub oauth_flows: Arc<OAuthFlowStore>,
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

impl AppState {
    /// Broadcast domain events to all connected SSE clients.
    ///
    /// @spec docs/L1-api#sse-event-stream
    /// @spec docs/L1-sync#event-propagation
    pub fn publish_events(&self, events: &[DomainEvent]) {
        for event in events {
            let _ = self.event_sender.send(event.clone());
        }
    }
}

/// Handle returned by [`start_server`]. Holds the bound address, the server
/// task, and the log guard that must survive for the process lifetime.
pub struct ServerHandle {
    pub addr: SocketAddr,
    pub join_handle: tokio::task::JoinHandle<()>,
    pub log_guard: WorkerGuard,
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
