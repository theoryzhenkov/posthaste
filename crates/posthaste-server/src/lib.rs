pub mod api;
pub mod auth;
pub mod authz;
pub mod config;
pub mod logging;
pub mod oauth;
pub mod observability;
pub mod openapi;
pub mod push;
pub mod sanitize;
pub mod secret;
pub mod supervisor;
pub mod token;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{middleware, Router};
#[cfg(debug_assertions)]
use dotenvy::dotenv;
use posthaste_config::TomlConfigRepository;
use posthaste_domain::{ConfigRepository, DomainEvent, MailService, MailStore, SecretStore};
use posthaste_observability::{events, ph_info};
use posthaste_store::DatabaseStore;
use tokio::sync::broadcast;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing::{field, info_span, Span};
use tracing_appender::non_blocking::WorkerGuard;

use crate::config::resolve_roots;
use crate::oauth::OAuthFlowStore;
use crate::secret::SystemSecretStore;
use crate::supervisor::AccountSupervisor;

const SEND_MESSAGE_BODY_LIMIT_BYTES: usize = 40 * 1024 * 1024;

/// Shared application state threaded through all Axum handlers.
///
/// @spec docs/L0-api#axum
/// @spec docs/L1-api#endpoint-table
mod app_state;
mod router;
mod secure_file;
mod spa;
mod startup;

pub use app_state::{AppState, ServerConfig, ServerHandle};
pub use router::build_api_router;
pub use secure_file::write_secure_file;
pub use startup::start_server;

pub(crate) use spa::spa_fallback_service;

#[cfg(test)]
mod tests;
