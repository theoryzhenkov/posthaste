//! The far server: the in-process bundled daemon + the standalone backend role.
//!
//! Layers the backend assembly (store/engine/imap via `posthaste-authority-runtime`)
//! and the OAuth provider-flow routes underneath the near `/v1` platform
//! (`posthaste-api`), and serves the runtime↔backend `link_router`. There is
//! no facade: consumers import the near `/v1` platform from `posthaste_api`
//! directly (RFC D19b); this crate exports only what it owns.

/// The bundled server's OpenAPI document = the near `/v1` platform + the OAuth
/// routes it serves on top (the lean near node serves the OAuth-free near doc).
pub mod openapi;

pub mod oauth_routes;

mod startup;
mod startup_backend;

/// The far-node link wire (`link_router` + `LinkAuth`) lives in
/// `posthaste-authority-runtime` with its own error/auth vocabulary (RFC D24):
/// the standalone far-node binary no longer drags the `/v1` client platform to
/// serve it. `posthaste-server` remains the composition root that mounts it.
use posthaste_authority_runtime::{link_router, LinkAuth};
pub use oauth_routes::{build_oauth_router, OAuthState};
pub use startup::start_server;
pub use startup_backend::{start_backend, BackendServerHandle};

// Far prelude: items the far modules reach through `use super::*`
// (`startup`, `startup_backend`).
use std::sync::Arc;

#[cfg(debug_assertions)]
use dotenvy::dotenv;
use posthaste_api::{
    assemble_daemon_preamble, build_api_router, build_app_state, serve, DaemonPreamble,
    ServeOptions, ServerConfig, ServerHandle,
};
use posthaste_authority_runtime::{build_authority_runtime, build_backend_node};
use posthaste_observability::{events, ph_info};
use posthaste_runtime::{build_remote_runtime, BackendTransportConfig};
use tower_http::trace::TraceLayer;
use tracing::{field, info_span, Span};
use tracing_appender::non_blocking::WorkerGuard;

#[cfg(test)]
mod tests;
