//! The far server: the in-process bundled daemon + the standalone backend role.
//!
//! Layers the backend assembly (store/engine/imap via `posthaste-authority-runtime`)
//! and the OAuth provider-flow routes underneath the near `/v1` platform
//! (`posthaste-api`), and serves the runtime↔backend `link_router`. The near
//! platform is re-exported so existing `crate::X` paths (and the integration
//! tests) keep resolving.

// Facade: re-export the near `/v1` platform so far code + tests keep their
// `crate::api`/`crate::auth`/… paths and the public `posthaste_server::` surface.
pub use posthaste_api::{
    api, assemble_daemon_preamble, auth, authz, build_api_router, build_app_state, config,
    logging, observability, resolve_roots, sanitize, secret, serve, token, write_secure_file,
    AppState, DaemonPreamble, ResolvedRoots, ServeOptions, ServerConfig, ServerHandle,
    SystemSecretStore,
};
/// Daemon config resolution is owned by `posthaste-config` (D25): the
/// `DaemonSettings` struct and `read_daemon_settings`/`load_daemon_settings`
/// resolution live there and no longer route through `posthaste-api`.
pub use posthaste_config::{read_daemon_settings, DaemonSettings};

/// The bundled server's OpenAPI document = the near `/v1` platform + the OAuth
/// routes it serves on top (the lean near node serves the OAuth-free near doc).
pub mod openapi;

/// Re-export of `posthaste_authority_runtime::oauth` for the OAuth provider flow.
pub mod oauth {
    pub use posthaste_authority_runtime::oauth::*;
}
/// Re-export of the account supervisor for migration/test harnesses.
pub mod supervisor {
    pub use posthaste_authority_runtime::supervisor::*;
}

pub mod oauth_routes;

mod startup;
mod startup_backend;

/// The far-node link wire (`link_router` + `LinkAuth`) lives in
/// `posthaste-authority-runtime` with its own error/auth vocabulary (RFC D24):
/// the standalone far-node binary no longer drags the `/v1` client platform to
/// serve it. `posthaste-server` remains the composition root that mounts it.
pub use posthaste_authority_runtime::{link_router, LinkAuth};
pub use oauth_routes::{build_oauth_router, OAuthState};
pub use startup::start_server;
pub use startup_backend::{start_backend, BackendServerHandle};

// Far prelude: items the far modules reach through `use super::*`
// (`startup`, `startup_backend`).
use std::sync::Arc;

#[cfg(debug_assertions)]
use dotenvy::dotenv;
use posthaste_authority_runtime::{
    build_authority_runtime, build_backend_node, build_remote_runtime, BackendTransportConfig,
};
use posthaste_observability::{events, ph_info};
use tower_http::trace::TraceLayer;
use tracing::{field, info_span, Span};
use tracing_appender::non_blocking::WorkerGuard;

#[cfg(test)]
mod tests;
