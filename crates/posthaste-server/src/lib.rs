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
    api, auth, authz, build_api_router, build_app_state, config, logging, observability,
    read_daemon_settings, resolve_roots, sanitize, secret, serve, token, write_secure_file,
    AppState, DaemonSettings, ResolvedRoots, ServeOptions, ServerConfig, ServerHandle,
    SystemSecretStore,
};

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

pub mod link;
pub mod oauth_routes;

mod migration;
mod startup;
mod startup_backend;

pub use link::{link_router, LinkAuth};
pub use migration::{
    runtime_handle_for_migration, runtime_handle_with_account_runtime_provider_for_migration,
};
pub use oauth_routes::{build_oauth_router, OAuthState};
pub use startup::start_server;
pub use startup_backend::{start_backend, BackendServerHandle};

// Far prelude: items the far modules reach through `use super::*`
// (`startup`, `startup_backend`).
use std::sync::Arc;
use std::time::Duration;

#[cfg(debug_assertions)]
use dotenvy::dotenv;
use posthaste_authority_runtime::{
    build_authority_runtime, build_backend_node, build_remote_runtime, AuthorityRuntimeBuildConfig,
    BackendTransportConfig,
};
use posthaste_config::TomlConfigRepository;
use posthaste_observability::{events, ph_info};
use tower_http::trace::TraceLayer;
use tracing::{field, info_span, Span};
use tracing_appender::non_blocking::WorkerGuard;

#[cfg(test)]
mod tests;
