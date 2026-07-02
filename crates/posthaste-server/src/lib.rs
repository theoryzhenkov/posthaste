//! The far server: the in-process bundled daemon + the standalone authority server role.
//!
//! Layers the authority server assembly (store/engine/imap via `posthaste-authority-server`)
//! and the OAuth provider-flow routes underneath the near `/v1` platform
//! (`posthaste-http-api-adapter`), and serves the runtime↔authority-server `link_router`. There is
//! no facade: consumers import the near `/v1` platform from `posthaste_http_api_adapter`
//! directly (RFC D19b); this crate exports only what it owns.

/// The bundled server's OpenAPI document = the near `/v1` platform + the OAuth
/// routes it serves on top (the lean near node serves the OAuth-free near doc).
pub mod openapi;

pub mod oauth_routes;

mod startup;
mod startup_authority_server;

/// The far-node link wire (`link_router` + `LinkAuth`) lives in
/// `posthaste-authority-server` with its own error/auth vocabulary (RFC D24):
/// the standalone far-node binary no longer drags the `/v1` client platform to
/// serve it. `posthaste-server` remains the composition root that mounts it.
use posthaste_authority_server::{link_router, LinkAuth};
pub use oauth_routes::{build_oauth_router, OAuthState};
pub use startup::start_server;
pub use startup_authority_server::{start_authority_server, AuthorityServerHandle};

// Far prelude: items the far modules reach through `use super::*`
// (`startup`, `startup_authority_server`).
use std::sync::Arc;

#[cfg(debug_assertions)]
use dotenvy::dotenv;
use posthaste_http_api_adapter::{
    assemble_daemon_preamble, build_api_router, build_app_state, serve, DaemonPreamble,
    ServeOptions, ServerConfig, ServerHandle,
};
use posthaste_authority_server::{build_authority_server, build_authority_server_node};
use posthaste_observability::{events, ph_info};
use posthaste_runtime::{build_remote_runtime, AuthorityServerTransportConfig};
use tower_http::trace::TraceLayer;
use tracing::{field, info_span, Span};
use tracing_appender::non_blocking::WorkerGuard;

#[cfg(test)]
mod tests;
