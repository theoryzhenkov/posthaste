//! The HTTP + SSE surface over [`AppState`]: `POST /query` (typed reads with
//! a generation stamp), `POST /command` (typed intents with idempotency ids),
//! `GET /events` (one SSE broadcast carrying the store generation and domain
//! events), and `GET /blobs/{id}` (attachment downloads). Every route is
//! also mounted under `/api` for the frontend's base path.
//!
//! Failures use the models error envelope; a provider failure after a command
//! was accepted is never an HTTP error — it surfaces later as the operation's
//! verdict through the pending-operations query.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::sse::{Event as SseEvent, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::Stream;
use posthaste_client_models::{
    AccountRow, AccountsResult, ApiError, ApiErrorKind, Command, CommandAccepted, CommandEnvelope,
    DomainEventPayload, EventMessage, MailListQuery, MailListResult, MailboxCountsQuery,
    MailboxCountsResult, MailboxCountsRow, PendingOperationRow, PendingOperationsResult, Query,
    QueryEnvelope,
};
use posthaste_domain_model::{
    AccountId, AccountSettings, BlobId, DomainEvent, Id, MailQueryCondition, MailQueryField,
    MailQueryGroup, MailQueryGroupOperator, MailQueryOperator, MailQueryRule, MailQueryRuleNode,
    MailQueryValue, MessageCursor, MessageId, OperationId, ServiceError, ServiceErrorKind,
    SortDirection, SyncTrigger, EVENT_TOPIC_ACCOUNT_CREATED, EVENT_TOPIC_ACCOUNT_UPDATED,
};
use tokio::sync::broadcast;

use crate::AppState;

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
struct ApiState {
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
        .route("/query", post(handle_query))
        .route("/command", post(handle_command))
        .route("/events", get(handle_events))
        .route("/blobs/{blob_id}", get(handle_blob_download))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES));
    Router::new()
        .merge(routes.clone())
        .nest("/api", routes)
        .with_state(state)
}

// ---------------------------------------------------------------- failures

/// A failed HTTP call: a status code plus the one models error envelope.
#[derive(Debug)]
struct ApiFailure {
    status: StatusCode,
    error: ApiError,
}

impl ApiFailure {
    fn new(
        status: StatusCode,
        kind: ApiErrorKind,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            status,
            error: ApiError {
                kind,
                message: message.into(),
                retryable,
            },
        }
    }

    fn malformed(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            ApiErrorKind::MalformedRequest,
            message,
            false,
        )
    }

    fn unknown_id(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            ApiErrorKind::UnknownId,
            message,
            false,
        )
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            ApiErrorKind::Unavailable,
            message,
            true,
        )
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorKind::Internal,
            message,
            false,
        )
    }
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> Response {
        (self.status, Json(self.error)).into_response()
    }
}

impl From<posthaste_domain_model::StoreError> for ApiFailure {
    fn from(error: posthaste_domain_model::StoreError) -> Self {
        Self::from(ServiceError::from(error))
    }
}

impl From<ServiceError> for ApiFailure {
    fn from(error: ServiceError) -> Self {
        let message = error.to_string();
        match error.kind() {
            ServiceErrorKind::NotFound => Self::unknown_id(message),
            ServiceErrorKind::Conflict | ServiceErrorKind::MailboxNotEmpty => {
                Self::new(StatusCode::CONFLICT, ApiErrorKind::Conflict, message, false)
            }
            ServiceErrorKind::ConfigValidation | ServiceErrorKind::ConfigParse => {
                Self::malformed(message)
            }
            ServiceErrorKind::GatewayUnavailable
            | ServiceErrorKind::NetworkError
            | ServiceErrorKind::SecretUnavailable => Self::unavailable(message),
            ServiceErrorKind::AuthError
            | ServiceErrorKind::StateMismatch
            | ServiceErrorKind::CannotCalculateChanges
            | ServiceErrorKind::GatewayRejected => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiErrorKind::Unavailable,
                message,
                false,
            ),
            ServiceErrorKind::StorageFailure | ServiceErrorKind::ConfigIo => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiErrorKind::Internal,
                message,
                true,
            ),
            ServiceErrorKind::StorageCorrupted
            | ServiceErrorKind::SecretUnsupported
            | ServiceErrorKind::Internal => Self::internal(message),
        }
    }
}

// -------------------------------------------------------------------- auth

/// Session-secret check on every route. Accepts the bearer header
/// everywhere, and the `?token=` query parameter only on the GETs whose
/// consumers cannot set headers (the browser `EventSource` on `/events`,
/// plain anchors on blob downloads) — so the credential stays out of URLs on
/// the query/command routes. The comparison is constant-time: the token
/// cannot be recovered byte-by-byte from response timing.
async fn require_auth(State(state): State<ApiState>, request: Request, next: Next) -> Response {
    let presented = bearer_token(request.headers()).or_else(|| {
        accepts_query_token(&request)
            .then(|| query_token(request.uri()))
            .flatten()
    });
    let authorized = presented
        .as_deref()
        .is_some_and(|token| constant_time_eq(token.as_bytes(), state.token.as_bytes()));
    if !authorized {
        return ApiFailure::new(
            StatusCode::UNAUTHORIZED,
            ApiErrorKind::Unauthorized,
            "missing or invalid session token",
            false,
        )
        .into_response();
    }
    next.run(request).await
}

/// The routes that accept `?token=`: `GET /events` and `GET /blobs/{id}`
/// (with or without the `/api` prefix).
fn accepts_query_token(request: &Request) -> bool {
    if request.method() != Method::GET {
        return false;
    }
    let path = request.uri().path();
    let path = path.strip_prefix("/api").unwrap_or(path);
    path == "/events" || path.starts_with("/blobs/")
}

/// Equality over every byte with no early exit. The comparison time leaks
/// only the length mismatch (the token length is fixed and public), never
/// which byte differs.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |acc, (l, r)| acc | (l ^ r))
        == 0
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::to_string)
}

fn query_token(uri: &Uri) -> Option<String> {
    uri.query()?
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
        .map(str::to_string)
}

fn decode_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ApiFailure> {
    serde_json::from_slice(body)
        .map_err(|error| ApiFailure::malformed(format!("invalid request body: {error}")))
}

// ----------------------------------------------------------------- queries

/// `POST /query`: evaluate one typed read over the effective views. The
/// generation is stamped BEFORE evaluation, so a write racing the read makes
/// the answer look older than the stream and the client refetches — staleness
/// always resolves toward a refetch, never a stuck view.
async fn handle_query(State(state): State<ApiState>, body: Bytes) -> Result<Response, ApiFailure> {
    let query: Query = decode_json(&body)?;
    let generation = state.app.events.generation();
    let data = evaluate_query(&state.app, query).await?;
    Ok(Json(QueryEnvelope { generation, data }).into_response())
}

async fn evaluate_query(app: &AppState, query: Query) -> Result<serde_json::Value, ApiFailure> {
    // Synchronous store reads go through `offload_read`: both the SQLite
    // evaluation and the pooled read-connection acquisition block the calling
    // thread, which must never be an async worker.
    let data = match query {
        Query::MailList(query) => {
            let app = app.clone();
            to_value(offload_read(move || evaluate_mail_list(&app, query)).await?)?
        }
        Query::Thread(query) => {
            let service = app.service.clone();
            to_value(
                offload_read(move || Ok(service.get_thread(&query.account_id, &query.thread_id)?))
                    .await?,
            )?
        }
        Query::MessageDetail(query) => to_value(evaluate_message_detail(app, query).await?)?,
        Query::MailboxCounts(query) => {
            let app = app.clone();
            to_value(offload_read(move || evaluate_mailbox_counts(&app, query)).await?)?
        }
        Query::Accounts(_) => to_value(evaluate_accounts(app).await?)?,
        Query::PendingOperations(query) => {
            let app = app.clone();
            to_value(offload_read(move || evaluate_pending_operations(&app, query)).await?)?
        }
    };
    Ok(data)
}

/// Run one synchronous store read on the blocking pool and hand its result
/// back to the async caller.
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

fn evaluate_mail_list(app: &AppState, query: MailListQuery) -> Result<MailListResult, ApiFailure> {
    let limit = query
        .limit
        .map(|limit| limit as usize)
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT);
    let cursor = query
        .cursor
        .as_deref()
        .map(parse_message_cursor)
        .transpose()?;
    let sort = query.sort.clone().unwrap_or_default();
    let direction = if sort.descending {
        SortDirection::Desc
    } else {
        SortDirection::Asc
    };
    let rule = mail_list_rule(&query);
    let page = app.service.query_message_page_by_rule(
        &rule,
        limit,
        cursor.as_ref(),
        sort.field,
        direction,
    )?;
    Ok(MailListResult {
        rows: page.items,
        next_cursor: page.next_cursor.as_ref().map(encode_message_cursor),
    })
}

/// Compile the mail-list filters into the shared mail-query AST: scope and
/// flag filters AND together; free text is an OR group over subject, sender
/// name/email, recipients, preview, and the cached body index.
fn mail_list_rule(query: &MailListQuery) -> MailQueryRule {
    fn condition(field: MailQueryField, value: MailQueryValue) -> MailQueryRuleNode {
        MailQueryRuleNode::Condition(MailQueryCondition {
            field,
            operator: MailQueryOperator::Equals,
            negated: false,
            value,
        })
    }

    let mut nodes = Vec::new();
    if let Some(account_id) = &query.account_id {
        nodes.push(condition(
            MailQueryField::SourceId,
            MailQueryValue::String(account_id.to_string()),
        ));
    }
    if let Some(mailbox_id) = &query.mailbox_id {
        nodes.push(condition(
            MailQueryField::MailboxId,
            MailQueryValue::String(mailbox_id.to_string()),
        ));
    }
    if let Some(is_read) = query.is_read {
        nodes.push(condition(
            MailQueryField::IsRead,
            MailQueryValue::Bool(is_read),
        ));
    }
    if let Some(is_flagged) = query.is_flagged {
        nodes.push(condition(
            MailQueryField::IsFlagged,
            MailQueryValue::Bool(is_flagged),
        ));
    }
    if let Some(has_attachment) = query.has_attachment {
        nodes.push(condition(
            MailQueryField::HasAttachment,
            MailQueryValue::Bool(has_attachment),
        ));
    }
    if let Some(text) = query
        .free_text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        let contains = |field| {
            MailQueryRuleNode::Condition(MailQueryCondition {
                field,
                operator: MailQueryOperator::Contains,
                negated: false,
                value: MailQueryValue::String(text.to_string()),
            })
        };
        nodes.push(MailQueryRuleNode::Group(MailQueryGroup {
            operator: MailQueryGroupOperator::Any,
            negated: false,
            nodes: vec![
                contains(MailQueryField::Subject),
                contains(MailQueryField::FromName),
                contains(MailQueryField::FromEmail),
                contains(MailQueryField::To),
                contains(MailQueryField::Preview),
                contains(MailQueryField::Body),
            ],
        }));
    }
    MailQueryRule {
        root: MailQueryGroup {
            operator: MailQueryGroupOperator::All,
            negated: false,
            nodes,
        },
    }
}

async fn evaluate_message_detail(
    app: &AppState,
    query: posthaste_client_models::MessageDetailQuery,
) -> Result<posthaste_client_models::MessageDetailResult, ApiFailure> {
    // The gateway is optional: connected accounts fetch a missing body
    // lazily; offline the cached projection serves.
    let gateway = app.supervisor.gateway(&query.account_id).await.ok();
    let result = app
        .service
        .get_message_detail(&query.account_id, &query.message_id, gateway.as_deref())
        .await?;
    // A lazy body fetch is a committed write: publish its events so other
    // clients observe the cache fill.
    app.events.publish(&result.events);
    let detail = result
        .detail
        .ok_or_else(|| ApiFailure::unknown_id(format!("message {}", query.message_id.as_str())))?;
    Ok(posthaste_client_models::MessageDetailResult {
        summary: detail.summary,
        body_html: detail.body_html,
        body_text: detail.body_text,
        attachments: detail.attachments,
        list_unsubscribe: detail.list_unsubscribe,
    })
}

fn evaluate_mailbox_counts(
    app: &AppState,
    query: MailboxCountsQuery,
) -> Result<MailboxCountsResult, ApiFailure> {
    let mut rows = Vec::new();
    for account_id in scoped_accounts(app, query.account_id.as_ref())? {
        let mut mailboxes = app.service.list_mailboxes(&account_id)?;
        // Display order is part of the answer — role mailboxes first in a
        // fixed precedence, then named folders by name — so the client
        // renders the rows verbatim instead of re-sorting a query answer.
        mailboxes.sort_by(|left, right| {
            mailbox_role_rank(left.role.as_deref())
                .cmp(&mailbox_role_rank(right.role.as_deref()))
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        rows.extend(mailboxes.into_iter().map(|mailbox| MailboxCountsRow {
            account_id: account_id.clone(),
            mailbox,
        }));
    }
    Ok(MailboxCountsResult { rows })
}

/// Fixed display precedence for role mailboxes; named folders follow, by
/// name.
fn mailbox_role_rank(role: Option<&str>) -> u8 {
    match role {
        Some("inbox") => 0,
        Some("drafts") => 1,
        Some("sent") => 2,
        Some("archive") => 3,
        Some("junk") => 4,
        Some("trash") => 5,
        _ => 6,
    }
}

async fn evaluate_accounts(app: &AppState) -> Result<AccountsResult, ApiFailure> {
    let service = app.service.clone();
    let (sources, default_account_id) = offload_read(move || {
        let sources = service.list_sources()?;
        let default_account_id = service.get_app_settings()?.default_account_id;
        Ok((sources, default_account_id))
    })
    .await?;
    let overviews = app.supervisor.runtime_overviews().await;
    let rows = sources
        .into_iter()
        .map(|source| {
            let overview = overviews
                .get(source.id.as_str())
                .cloned()
                .unwrap_or_default();
            AccountRow {
                is_default: default_account_id.as_ref() == Some(&source.id),
                id: source.id,
                name: source.name,
                full_name: source.full_name,
                enabled: source.enabled,
                status: overview.status,
                push: overview.push,
                last_sync_at: overview.last_sync_at,
                last_sync_error: overview.last_sync_error,
            }
        })
        .collect();
    Ok(AccountsResult { rows })
}

fn evaluate_pending_operations(
    app: &AppState,
    query: posthaste_client_models::PendingOperationsQuery,
) -> Result<PendingOperationsResult, ApiFailure> {
    let mut rows = Vec::new();
    for account_id in scoped_accounts(app, query.account_id.as_ref())? {
        for operation in app.service.list_pending_operations(&account_id)? {
            rows.push(PendingOperationRow {
                id: operation.id,
                account_id: operation.account_id,
                kind: operation.kind,
                state: operation.state,
                entity_kind: operation.entity.kind,
                entity_id: operation.entity.id,
                attempts: operation.attempts,
                last_error: operation.last_error,
                send_at: operation.send_at,
                created_at: operation.created_at,
                updated_at: operation.updated_at,
            });
        }
    }
    // Newest first across accounts; created_at is normalized RFC 3339, so
    // the lexicographic order is the chronological order.
    rows.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(PendingOperationsResult { rows })
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

// ----------------------------------------------------------------- cursors

/// Opaque mail-list cursor codec:
/// `{sort_len}:{sort_value}:{source_len}:{source_id}:{message_id}`.
fn encode_message_cursor(cursor: &MessageCursor) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        cursor.sort_value.len(),
        cursor.sort_value,
        cursor.source_id.as_str().len(),
        cursor.source_id.as_str(),
        cursor.message_id.as_str()
    )
}

fn parse_message_cursor(cursor: &str) -> Result<MessageCursor, ApiFailure> {
    fn take_prefixed(value: &str) -> Option<(&str, &str)> {
        let (len_prefix, remainder) = value.split_once(':')?;
        let value_len = len_prefix.parse::<usize>().ok()?;
        // The length is client-supplied bytes: reject it unless it lands on
        // a char boundary of the remainder (`split_at` panics otherwise —
        // sort values legitimately carry multi-byte UTF-8).
        if remainder.len() <= value_len || !remainder.is_char_boundary(value_len) {
            return None;
        }
        let (prefixed, remainder) = remainder.split_at(value_len);
        Some((prefixed, remainder.strip_prefix(':')?))
    }

    let invalid = || ApiFailure::malformed("malformed mail-list cursor");
    let (sort_value, remainder) = take_prefixed(cursor).ok_or_else(invalid)?;
    let (source_id, message_id) = take_prefixed(remainder).ok_or_else(invalid)?;
    if source_id.is_empty() || message_id.is_empty() {
        return Err(invalid());
    }
    Ok(MessageCursor {
        sort_value: sort_value.to_string(),
        source_id: AccountId::from(source_id),
        message_id: MessageId::from(message_id),
    })
}

// ---------------------------------------------------------------- commands

/// `POST /command`: apply one typed intent. Replaying an id returns an
/// outcome without re-applying: concurrent and in-run retries resolve
/// against the per-id outcome cell, and a send retry that outlives the
/// process resolves against the outbox, whose operation id for a send is
/// the command id itself.
async fn handle_command(
    State(state): State<ApiState>,
    body: Bytes,
) -> Result<Json<CommandAccepted>, ApiFailure> {
    let envelope: CommandEnvelope = decode_json(&body)?;
    if envelope.id.trim().is_empty() {
        return Err(ApiFailure::malformed("command id must not be empty"));
    }
    let cell = command_outcome_cell(&state, &envelope.id);
    let mut outcome = cell.lock().await;
    if let Some(generation) = *outcome {
        return Ok(Json(CommandAccepted { generation }));
    }
    if let Some(generation) = replay_durable_outcome(&state, &envelope).await? {
        *outcome = Some(generation);
        return Ok(Json(CommandAccepted { generation }));
    }
    let generation = apply_command(&state.app, &envelope.id, envelope.command).await?;
    *outcome = Some(generation);
    Ok(Json(CommandAccepted { generation }))
}

/// The outcome cell for one command id. The map lock is held only for the
/// lookup, so distinct commands execute concurrently; retries of one id
/// share the cell and serialize on it. Once the map outgrows its cap,
/// settled cells are evicted (an in-flight cell is also held by its
/// executing request, so it survives the sweep).
fn command_outcome_cell(state: &ApiState, id: &str) -> CommandOutcome {
    let mut outcomes = state
        .command_outcomes
        .lock()
        .expect("command-outcome map lock poisoned");
    if outcomes.len() >= COMMAND_OUTCOME_CAP && !outcomes.contains_key(id) {
        outcomes.retain(|_, cell| Arc::strong_count(cell) > 1);
    }
    outcomes.entry(id.to_string()).or_default().clone()
}

/// Durable replay detection for a send: its outbox operation id is the
/// command id, so a replayed id whose operation the outbox still holds is
/// answered from the stored intent without enqueuing a second send. The
/// current generation is at or past the original acceptance, which is all
/// the reply promises.
async fn replay_durable_outcome(
    state: &ApiState,
    envelope: &CommandEnvelope,
) -> Result<Option<u64>, ApiFailure> {
    if !matches!(envelope.command, Command::Send(_)) {
        return Ok(None);
    }
    let service = state.app.service.clone();
    let operation_id = OperationId::from(envelope.id.as_str());
    let existing = offload_read(move || Ok(service.get_operation(&operation_id)?)).await?;
    Ok(existing.map(|_| state.app.events.generation()))
}

async fn apply_command(
    app: &AppState,
    command_id: &str,
    command: Command,
) -> Result<u64, ApiFailure> {
    match command {
        Command::SetKeywords(intent) => {
            let ack = app
                .service
                .set_keywords(&intent.account_id, &intent.message_id, &intent.change)
                .await?;
            Ok(finish_mail_command(app, &intent.account_id, ack.events).await)
        }
        Command::ReplaceMailboxes(intent) => {
            let ack = app
                .service
                .replace_mailboxes(&intent.account_id, &intent.message_id, &intent.change)
                .await?;
            Ok(finish_mail_command(app, &intent.account_id, ack.events).await)
        }
        Command::Destroy(intent) => {
            let ack = app
                .service
                .destroy_message(&intent.account_id, &intent.message_id)
                .await?;
            Ok(finish_mail_command(app, &intent.account_id, ack.events).await)
        }
        Command::CreateDraft(intent) => {
            let draft_key = intent
                .draft
                .draft_id
                .as_deref()
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(MessageId::from);
            let (_, events) = app
                .service
                .save_draft(&intent.account_id, draft_key, intent.draft)
                .await?;
            Ok(finish_mail_command(app, &intent.account_id, events).await)
        }
        Command::UpdateDraft(intent) => {
            let (_, events) = app
                .service
                .save_draft(
                    &intent.account_id,
                    Some(MessageId::from(intent.draft_id.as_str())),
                    intent.draft,
                )
                .await?;
            Ok(finish_mail_command(app, &intent.account_id, events).await)
        }
        Command::DiscardDraft(intent) => {
            let ack = app
                .service
                .discard_draft(
                    &intent.account_id,
                    MessageId::from(intent.draft_id.as_str()),
                )
                .await?;
            Ok(finish_mail_command(app, &intent.account_id, ack.events).await)
        }
        Command::Send(intent) => {
            // The command id becomes the outbox operation id, so the intent
            // id is the send's idempotency key end to end — a replayed id
            // can never enqueue a second dispatch.
            let (_, events) = app
                .service
                .enqueue_send_with_operation_id(
                    &intent.account_id,
                    intent.request,
                    Some(OperationId::from(command_id)),
                )
                .await?;
            Ok(finish_mail_command(app, &intent.account_id, events).await)
        }
        Command::CreateAccount(intent) => create_account(app, intent).await,
        Command::UpdateAccount(intent) => update_account(app, intent).await,
    }
}

/// Publish a mail command's committed-write events (bumping the generation;
/// an event-less commit still bumps) and nudge the account runtime so the
/// queued operation flushes promptly. A missing runtime is fine — the op is
/// durable and flushes on the next sync window.
async fn finish_mail_command(
    app: &AppState,
    account_id: &AccountId,
    events: Vec<DomainEvent>,
) -> u64 {
    let generation = if events.is_empty() {
        app.events.bump()
    } else {
        app.events.publish(&events);
        app.events.generation()
    };
    let _ = app
        .supervisor
        .trigger_account_sync(account_id, SyncTrigger::Manual)
        .await;
    generation
}

async fn create_account(
    app: &AppState,
    intent: posthaste_client_models::CreateAccountIntent,
) -> Result<u64, ApiFailure> {
    let name = intent.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiFailure::malformed("account name must not be empty"));
    }
    let now = now_rfc3339();
    let settings = AccountSettings {
        id: AccountId::from(Id::generate()),
        name,
        full_name: intent.full_name,
        signature: intent.signature,
        email_patterns: intent.email_patterns,
        driver: posthaste_domain_model::AccountDriver::ImapSmtp,
        // A new account starts disabled unless asked otherwise: its
        // connection details are configured through the settings surface,
        // and an enabled account without them would only report errors.
        enabled: intent.enabled.unwrap_or(false),
        appearance: None,
        transport: posthaste_domain_model::AccountTransportSettings::default(),
        created_at: now.clone(),
        updated_at: now,
    };
    app.service.insert_source(&settings)?;
    app.supervisor.start_account(&settings).await;
    Ok(publish_account_event(
        app,
        &settings.id,
        EVENT_TOPIC_ACCOUNT_CREATED,
    ))
}

async fn update_account(
    app: &AppState,
    intent: posthaste_client_models::UpdateAccountIntent,
) -> Result<u64, ApiFailure> {
    let mut settings = app
        .service
        .get_source(&intent.account_id)?
        .ok_or_else(|| ApiFailure::unknown_id(format!("account {}", intent.account_id.as_str())))?;
    if let Some(name) = intent.name {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(ApiFailure::malformed("account name must not be empty"));
        }
        settings.name = name;
    }
    if let Some(full_name) = intent.full_name {
        settings.full_name = Some(full_name);
    }
    if let Some(signature) = intent.signature {
        settings.signature = Some(signature);
    }
    if let Some(email_patterns) = intent.email_patterns {
        settings.email_patterns = email_patterns;
    }
    if let Some(enabled) = intent.enabled {
        settings.enabled = enabled;
    }
    settings.updated_at = now_rfc3339();
    app.service.save_source(&settings)?;
    // Restart (or park) the runtime under the new settings.
    app.supervisor.start_account(&settings).await;
    Ok(publish_account_event(
        app,
        &settings.id,
        EVENT_TOPIC_ACCOUNT_UPDATED,
    ))
}

/// Publish an account-configuration event so every connected client observes
/// the change on the stream (bumping the generation), and return the
/// resulting generation.
fn publish_account_event(app: &AppState, account_id: &AccountId, topic: &str) -> u64 {
    app.events.publish(&[DomainEvent {
        seq: 0,
        account_id: account_id.clone(),
        topic: topic.to_string(),
        occurred_at: now_rfc3339(),
        mailbox_id: None,
        message_id: None,
        payload: serde_json::Value::Null,
    }]);
    app.events.generation()
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

// ------------------------------------------------------------------- blobs

/// `GET /blobs/{blob_id}`: serve an attachment blob through the owning
/// account's gateway (cached raw bytes short-circuit the provider call
/// inside the service). Blobs are immutable, so the response carries
/// long-lived caching headers.
async fn handle_blob_download(
    State(state): State<ApiState>,
    Path(blob_id): Path<String>,
) -> Result<Response, ApiFailure> {
    let blob = BlobId::from(blob_id.as_str());
    let store = state.app.database_store.clone();
    let lookup = blob.clone();
    let Some((account_id, message_id, attachment)) =
        offload_read(move || Ok(store.find_attachment_by_blob(&lookup)?)).await?
    else {
        return Err(ApiFailure::unknown_id(format!("blob {blob_id}")));
    };
    let gateway = state
        .app
        .supervisor
        .gateway(&account_id)
        .await
        .map_err(|_| {
            ApiFailure::unavailable(format!(
                "account {} is not connected; the attachment cannot be fetched right now",
                account_id.as_str()
            ))
        })?;
    let bytes = state
        .app
        .service
        .download_blob(&account_id, &message_id, &blob, gateway.as_ref())
        .await?;
    Ok(blob_response(bytes, &attachment.mime_type))
}

fn blob_response(bytes: Vec<u8>, mime_type: &str) -> Response {
    (
        [
            (header::CONTENT_TYPE, mime_type.to_string()),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable".to_string(),
            ),
        ],
        bytes,
    )
        .into_response()
}

// ------------------------------------------------------------ event stream

/// `GET /events`: the one SSE broadcast. Every message carries the current
/// store generation; most also carry a domain event. The first message is
/// the handshake and carries the run id, so a client detects a backend
/// restart (fresh run id = everything held is stale). A generation-only
/// heartbeat fills silences, and a lagged subscriber heals through a
/// generation-only message — payloads are prompts, never a ledger.
async fn handle_events(
    State(state): State<ApiState>,
) -> Sse<impl Stream<Item = Result<SseEvent, std::convert::Infallible>>> {
    let events = state.app.events;
    let mut receiver = events.subscribe();
    let stream = async_stream::stream! {
        yield Ok(sse_message(&EventMessage {
            generation: events.generation(),
            run_id: Some(events.run_id().to_string()),
            event: None,
        }));
        loop {
            let received = tokio::select! {
                () = tokio::time::sleep(HEARTBEAT_INTERVAL) => None,
                received = receiver.recv() => Some(received),
            };
            match received {
                // Silence: generation-only heartbeat.
                None => {
                    yield Ok(sse_message(&EventMessage {
                        generation: events.generation(),
                        run_id: None,
                        event: None,
                    }));
                }
                Some(Ok(first)) => {
                    // Coalesce the burst: gather everything arriving within
                    // the window, then flush one write batch stamped with
                    // the current generation.
                    let mut batch = vec![first];
                    let window = tokio::time::sleep(COALESCE_WINDOW);
                    tokio::pin!(window);
                    let mut closed = false;
                    loop {
                        tokio::select! {
                            () = &mut window => break,
                            received = receiver.recv() => match received {
                                Ok(event) => batch.push(event),
                                Err(broadcast::error::RecvError::Lagged(_)) => break,
                                Err(broadcast::error::RecvError::Closed) => {
                                    closed = true;
                                    break;
                                }
                            }
                        }
                    }
                    let generation = events.generation();
                    for event in batch {
                        yield Ok(sse_message(&EventMessage {
                            generation,
                            run_id: None,
                            event: Some(event_payload(event)),
                        }));
                    }
                    if closed {
                        break;
                    }
                }
                // Lagged: dropped payloads heal through the level-triggered
                // generation; the client refetches what it needs.
                Some(Err(broadcast::error::RecvError::Lagged(_))) => {
                    yield Ok(sse_message(&EventMessage {
                        generation: events.generation(),
                        run_id: None,
                        event: None,
                    }));
                }
                Some(Err(broadcast::error::RecvError::Closed)) => break,
            }
        }
    };
    Sse::new(stream)
}

fn sse_message(message: &EventMessage) -> SseEvent {
    SseEvent::default().data(serde_json::to_string(message).unwrap_or_default())
}

/// Map a domain event onto the wire payload: the topic plus scope ids, with
/// the kind-specific payload passed through verbatim.
fn event_payload(event: DomainEvent) -> DomainEventPayload {
    DomainEventPayload {
        kind: event.topic,
        account_id: event.account_id,
        message_id: event.message_id,
        mailbox_id: event.mailbox_id,
        payload: match event.payload {
            serde_json::Value::Null => None,
            payload => Some(payload),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_cursor_round_trips_multibyte_sort_values() {
        let cursor = MessageCursor {
            sort_value: "Résumé — überprüfen".to_string(),
            source_id: AccountId::from("acct-1"),
            message_id: MessageId::from("msg-1"),
        };
        let parsed = parse_message_cursor(&encode_message_cursor(&cursor)).expect("parses back");
        assert_eq!(parsed.sort_value, cursor.sort_value);
        assert_eq!(parsed.source_id.as_str(), "acct-1");
        assert_eq!(parsed.message_id.as_str(), "msg-1");
    }

    #[test]
    fn malformed_cursors_are_rejected_without_panicking() {
        for cursor in [
            "",
            "no-len",
            "9:short",
            "1:\u{e9}:1:a:b", // the length lands inside a multi-byte char
            "1:x:999:a:b",
            "0::0::",
        ] {
            assert!(parse_message_cursor(cursor).is_err(), "cursor {cursor:?}");
        }
    }

    #[test]
    fn constant_time_eq_matches_plain_equality() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(constant_time_eq(b"", b""));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secre"));
        assert!(!constant_time_eq(b"", b"x"));
    }

    #[test]
    fn mailbox_role_rank_orders_roles_before_named_folders() {
        assert!(mailbox_role_rank(Some("inbox")) < mailbox_role_rank(Some("drafts")));
        assert!(mailbox_role_rank(Some("drafts")) < mailbox_role_rank(Some("sent")));
        assert!(mailbox_role_rank(Some("trash")) < mailbox_role_rank(Some("Projects")));
        assert_eq!(mailbox_role_rank(None), mailbox_role_rank(Some("unknown")));
    }
}
