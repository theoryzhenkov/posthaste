//! The client-facing `/v1` HTTP platform.
//!
//! The REST resource API + the client↔runtime link (runtime sessions/views/
//! streams), the macaroon capability-token perimeter, HTML sanitization,
//! OpenAPI, and the serving glue. It drives the runtime over `posthaste-runtime`
//! (the near node) and never links the far-node roles (store/engine/imap), so a
//! lean remote runtime daemon can serve it. The OAuth-flow routes — which need
//! the backend's provider machinery — live in `posthaste-server`, layered on top
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
pub mod token;

mod app_state;
mod router;
mod secure_file;
mod serve;
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
use posthaste_runtime::{AuthorityRuntimeHandle, RuntimeShutdownHandle};
use tower_http::services::ServeDir;
use tracing_appender::non_blocking::WorkerGuard;

const SEND_MESSAGE_BODY_LIMIT_BYTES: usize = 40 * 1024 * 1024;

pub use app_state::{AppState, ServerConfig, ServerHandle};
pub use config::{
    load_daemon_settings, read_daemon_settings, resolve_roots, DaemonSettings, ResolvedRoots,
};
pub use router::build_api_router;
pub use secret::SystemSecretStore;
pub use secure_file::write_secure_file;
pub use serve::{build_app_state, serve, ServeOptions};

pub(crate) use spa::spa_fallback_service;

#[cfg(test)]
mod tests;
