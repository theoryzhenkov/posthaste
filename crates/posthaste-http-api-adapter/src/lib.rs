//! The client-facing `/v1` HTTP platform.
//!
//! The REST resource API + the client↔runtime link (runtime links/views/
//! streams), the macaroon capability-token perimeter, HTML sanitization,
//! OpenAPI, and the serving glue. It drives the runtime over `posthaste-runtime`
//! (the near node) and never links the far-node roles (store/engine/imap), so a
//! lean remote runtime daemon can serve it. The OAuth-flow routes — which need
//! the authority server's provider machinery — live in `posthaste-server`, layered on top
//! via [`build_api_router`] + a merged OAuth router.

pub mod api;
pub mod auth;
pub mod authz;
pub mod config;
pub mod logging;
pub mod observability;
pub mod openapi;
pub mod sanitize;
pub mod secret;
pub mod tls;
pub mod token;

mod app_state;
mod deadlines;
mod discovery;
mod router;
mod secure_file;
mod serve;
mod shutdown;
mod spa;

// Crate-root prelude: items the moved modules reach through `use super::*`
// (`router`, `app_state`, `spa`) and bare paths.
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{middleware, Router};
use posthaste_runtime::{RuntimeHandle, RuntimeShutdownHandle};
use tower_http::services::ServeDir;
use tracing_appender::non_blocking::WorkerGuard;

const SEND_MESSAGE_BODY_LIMIT_BYTES: usize = 40 * 1024 * 1024;

pub use app_state::{AppState, ServerConfig, ServerHandle};
pub use config::{resolve_roots, ResolvedRoots};
pub use discovery::{
    discovery_file_path, remove_discovery_file, write_discovery_file, DISCOVERY_FILE_VERSION,
};
pub use router::build_api_router;
pub use secret::SystemSecretStore;
pub use secure_file::write_secure_file;
pub use serve::{assemble_daemon_preamble, build_app_state, serve, DaemonPreamble, ServeOptions};
pub use shutdown::{
    wait_for_shutdown_signal, ShutdownSequence, StoreClose, SupervisorStop,
    HTTP_DRAIN_DEADLINE, STORE_CLOSE_DEADLINE, SUPERVISOR_STOP_DEADLINE, TOTAL_SHUTDOWN_BUDGET,
};

pub(crate) use spa::spa_fallback_service;

#[cfg(test)]
mod tests;
