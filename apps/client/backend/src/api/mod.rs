//! The HTTP + SSE surface over [`AppState`]: `POST /query` (typed reads with
//! a generation stamp), `POST /command` (typed intents with idempotency ids),
//! `GET /events` (one SSE broadcast carrying the store generation and domain
//! events), `GET /blobs/{id}` (attachment downloads), and
//! `GET /account-assets/logos/{id}` (account logo images). Every route is
//! also mounted under `/api` for the frontend's base path.
//!
//! One submodule per protocol family: each family's query evaluators and
//! command appliers live together in its module, and the [`query`] /
//! [`command`] dispatchers only route to them.
//!
//! Failures use the models error envelope; a provider failure after a command
//! was accepted is never an HTTP error — it surfaces later as the operation's
//! verdict through the pending-operations query.

mod accounts;
mod auth;
mod automation;
mod blobs;
mod command;
mod compose;
mod events;
mod failure;
mod mail_list;
mod mail_mutations;
mod mailboxes;
mod message_detail;
pub(crate) mod oauth;
mod operations;
mod query;
mod rev_log;
mod settings;
mod smart_mailboxes;
mod snooze;
mod sync;
mod tags;
mod thread;
mod unsubscribe;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use posthaste_domain_model::AccountId;

use crate::AppState;
pub(crate) use failure::ApiFailure;

/// Default page size for a mail list when the query carries no limit.
const DEFAULT_LIST_LIMIT: usize = 50;

/// Hard cap for a mail list page.
const MAX_LIST_LIMIT: usize = 200;

/// Maximum accepted request body (JSON commands, including base64-encoded
/// compose attachments inside a send request).
const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

/// Generation-only heartbeat interval on the event stream.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Burst-coalescing window on the event stream: events arriving within this
/// window of the first one are flushed together in one write batch.
const COALESCE_WINDOW: Duration = Duration::from_millis(100);

/// Cap on the per-run command-outcome cache; settled entries are evicted
/// once the map outgrows it.
const COMMAND_OUTCOME_CAP: usize = 4096;

/// Idempotency state for one command id: an in-flight guard plus the
/// recorded outcome generation. Concurrent retries of one id serialize on
/// the cell's lock and the losers read the recorded outcome; distinct ids
/// run concurrently.
type CommandOutcome = Arc<tokio::sync::Mutex<Option<u64>>>;

/// Shared state of the API layer: the assembled service core, the session
/// token, and the per-run command-outcome cache.
#[derive(Clone)]
pub(crate) struct ApiState {
    app: AppState,
    token: Arc<str>,
    /// Idempotency: an outcome cell per command id, for this run. The
    /// recorded generation is run-scoped (it resets with the process), so a
    /// run-scoped cache is the truthful store for it. The durable half lives
    /// in the outbox: a send's operation id IS its command id, so a replay
    /// that outlives the process is deduplicated against the stored intent.
    command_outcomes: Arc<std::sync::Mutex<HashMap<String, CommandOutcome>>>,
}

/// The API router over the service core. `token` is the session secret every
/// request must present (bearer header, or `?token=` for consumers that
/// cannot set headers, such as `EventSource` and plain download anchors).
pub fn build_router(app: AppState, token: String) -> Router {
    let state = ApiState {
        app,
        token: token.into(),
        command_outcomes: Arc::new(std::sync::Mutex::new(HashMap::new())),
    };
    let routes = Router::new()
        .route("/query", post(query::handle_query))
        .route("/command", post(command::handle_command))
        .route("/events", get(events::handle_events))
        .route("/blobs/{blob_id}", get(blobs::handle_blob_download))
        .route(
            "/account-assets/logos/{image_id}",
            get(accounts::handle_logo_download),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES));
    Router::new()
        .merge(routes.clone())
        .nest("/api", routes)
        .with_state(state)
}

fn decode_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ApiFailure> {
    serde_json::from_slice(body)
        .map_err(|error| ApiFailure::malformed(format!("invalid request body: {error}")))
}

/// Run one synchronous store read on the blocking pool and hand its result
/// back to the async caller. Both the SQLite evaluation and the pooled
/// read-connection acquisition block the calling thread, which must never be
/// an async worker.
async fn offload_read<T, F>(read: F) -> Result<T, ApiFailure>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ApiFailure> + Send + 'static,
{
    tokio::task::spawn_blocking(read)
        .await
        .map_err(|error| ApiFailure::internal(format!("read task failed: {error}")))?
}

fn to_value<T: serde::Serialize>(value: T) -> Result<serde_json::Value, ApiFailure> {
    serde_json::to_value(value)
        .map_err(|error| ApiFailure::internal(format!("failed to encode answer: {error}")))
}

/// Resolve the account scope of a query: one validated account, or every
/// configured one.
fn scoped_accounts(
    app: &AppState,
    account_id: Option<&AccountId>,
) -> Result<Vec<AccountId>, ApiFailure> {
    match account_id {
        Some(account_id) => {
            if app.service.get_source(account_id)?.is_none() {
                return Err(ApiFailure::unknown_id(format!(
                    "account {}",
                    account_id.as_str()
                )));
            }
            Ok(vec![account_id.clone()])
        }
        None => Ok(app
            .service
            .list_sources()?
            .into_iter()
            .map(|source| source.id)
            .collect()),
    }
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
