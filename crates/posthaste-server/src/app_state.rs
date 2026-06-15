use super::*;

pub struct AppState {
    /// Target runtime boundary for `/v1` mail behavior.
    ///
    /// MIGRATION(api-runtime-wrapper): existing handlers still use the legacy
    /// fields below while methods move onto `AuthorityRuntimeHandle`.
    ///
    /// @spec docs/eph/PLAN-L3-api-runtime-wrapper-migration#appstate-has-runtime-handle
    pub runtime: AuthorityRuntimeHandle,
    /// MIGRATION(api-runtime-wrapper): temporary direct service access for
    /// handlers that have not yet moved behind the runtime handle.
    ///
    /// @spec docs/eph/PLAN-L3-api-runtime-wrapper-migration#legacy-fields-temporary
    pub service: Arc<MailService>,
    /// MIGRATION(api-runtime-wrapper): temporary direct store access for
    /// handlers and test harnesses not yet moved behind runtime read methods.
    ///
    /// @spec docs/eph/PLAN-L3-api-runtime-wrapper-migration#legacy-fields-temporary
    pub store: Arc<dyn MailStore>,
    /// MIGRATION(api-runtime-wrapper): temporary direct secret-store access for
    /// account/OAuth handlers until runtime account methods own it.
    ///
    /// @spec docs/eph/PLAN-L3-api-runtime-wrapper-migration#legacy-fields-temporary
    pub secret_store: Arc<dyn SecretStore>,
    /// MIGRATION(api-runtime-wrapper): supervisor stays server-owned until its
    /// OAuth/push/provider wiring is extracted into the authority runtime.
    ///
    /// @spec docs/backend/L3#supervisor-server-owned-temporary
    pub supervisor: Arc<AccountSupervisor>,
    /// MIGRATION(api-runtime-wrapper): temporary direct event bus access for
    /// `/v1/events` until it consumes runtime event methods.
    ///
    /// @spec docs/eph/PLAN-L3-api-runtime-wrapper-migration#legacy-fields-temporary
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
    /// MIGRATION(api-runtime-wrapper): build a runtime handle around existing
    /// test/API parts while route handlers are incrementally moved behind the
    /// authority runtime.
    ///
    /// @spec docs/eph/PLAN-L3-api-runtime-wrapper-migration#appstate-has-runtime-handle
    pub fn runtime_handle_for_migration(
        service: Arc<MailService>,
        store: Arc<dyn MailStore>,
        secret_store: Arc<dyn SecretStore>,
        event_sender: broadcast::Sender<DomainEvent>,
    ) -> AuthorityRuntimeHandle {
        Self::runtime_handle_with_status_provider_for_migration(
            service,
            store,
            secret_store,
            event_sender,
            Arc::new(DefaultAccountRuntimeStatusProvider),
        )
    }

    pub fn runtime_handle_with_status_provider_for_migration(
        service: Arc<MailService>,
        store: Arc<dyn MailStore>,
        secret_store: Arc<dyn SecretStore>,
        event_sender: broadcast::Sender<DomainEvent>,
        status_provider: Arc<dyn AccountRuntimeOverviewProvider>,
    ) -> AuthorityRuntimeHandle {
        let account_count = service
            .list_sources()
            .expect("migration runtime handle should read configured sources")
            .len();
        AuthorityRuntimeHandle::from_api_bridge_with_status_provider_for_migration(
            AuthorityRuntimeApiMigrationBridge::new(service, store, secret_store, event_sender),
            account_count,
            status_provider,
        )
    }

    pub fn runtime_handle_with_account_runtime_provider_for_migration(
        service: Arc<MailService>,
        store: Arc<dyn MailStore>,
        secret_store: Arc<dyn SecretStore>,
        event_sender: broadcast::Sender<DomainEvent>,
        account_runtime_provider: Arc<AccountSupervisor>,
    ) -> AuthorityRuntimeHandle {
        let account_count = service
            .list_sources()
            .expect("migration runtime handle should read configured sources")
            .len();
        AuthorityRuntimeHandle::from_api_bridge_with_account_supervisor_for_migration(
            AuthorityRuntimeApiMigrationBridge::new(service, store, secret_store, event_sender),
            account_count,
            account_runtime_provider,
        )
    }

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

struct DefaultAccountRuntimeStatusProvider;

#[async_trait::async_trait]
impl AccountRuntimeOverviewProvider for DefaultAccountRuntimeStatusProvider {
    async fn runtime_overview(&self, _account_id: &AccountId) -> AccountRuntimeOverview {
        AccountRuntimeOverview::default()
    }
}

/// Handle returned by [`start_server`]. Holds the bound address, the server
/// task, and the log guard that must survive for the process lifetime.
pub struct ServerHandle {
    pub addr: SocketAddr,
    pub join_handle: tokio::task::JoinHandle<()>,
    pub log_guard: WorkerGuard,
    /// Owns graceful runtime shutdown for the server process.
    ///
    /// @spec docs/runtime/L2#runtime-shutdown-handle
    pub runtime_shutdown: RuntimeShutdownHandle,
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
