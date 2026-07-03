//! The authority server far-node HTTP surface: serves the runtime↔authority-server link wire.
//!
//! The symmetric inner twin of the client↔runtime API. Where that surface lets
//! a remote client drive the runtime, this one lets a **remote runtime** drive
//! the **authority server** ([replication authority-server-link L2 §3](../replication/authority-server-link/L2.md)): the up-channel is
//! a `POST` of a named mutation, the down-channel an SSE stream of base-assertion
//! frames. A split-authority-server host mounts this over the authority server's in-process
//! `AuthorityServerLinkHandle` transport (`AuthorityServerBuild::authority_server_link`); a runtime
//! configured with `transport = "remote"` connects to it via `RemoteTransport`.
//!
//! The wire paths and frame types are the shared contract
//! ([`posthaste_authority_server_link`]) — one definition, both ends — so client and
//! server cannot drift (assertion `one-link-transport`).
//!
//! This is the far node's own wire: it lives here (locality) with its own
//! minimal error/auth vocabulary, so the standalone far-node binary does not
//! drag the `/v1` client platform (`posthaste-http-api-adapter`) to serve it. The wire's
//! errors are its own — [`LinkError`] mirrors the `/v1` error envelope shape so
//! the remote runtime client sees the same response across the in-process hop,
//! but the type is local and narrow.
//!
//! @spec docs/replication/authority-server-link/L2#3-the-link-wire-link_router

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Extension, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{from_fn, from_fn_with_state, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use tower_http::timeout::TimeoutLayer;
use posthaste_config::DaemonSettings;
use posthaste_domain_model::{
    AccountId, ConversationId, ConversationView, MessageDetail, MessageId, MessageSummary,
};
use posthaste_domain_model::CommandAck;
use posthaste_authority_server_link::{
    AddToMailboxRequest, AuthorityServerApi, AuthorityServerLink,
    AuthorityServerLinkHandle, AuthorityServerLinkId, DestroyMessageRequest, LinkCoverage,
    RemoveFromMailboxRequest, ReplaceMailboxesRequest, SequencedFrame, SetKeywordsRequest,
    LINK_ADD_TO_MAILBOX_PATH, LINK_CONVERSATION_PATH, LINK_DESTROY_MESSAGE_PATH, LINK_DETAIL_PATH,
    LINK_FORWARD_MUTATION_PATH, LINK_QUERY_PATH, LINK_REMOVE_FROM_MAILBOX_PATH,
    LINK_REPLACE_MAILBOXES_PATH, LINK_SET_KEYWORDS_PATH, LINK_SUBSCRIBE_PATH, LINK_SUMMARY_PATH,
};
use posthaste_contract_core::{
    MailQueryPage, MailQueryRequest, MutationReceipt, MutationRequest, RuntimeAdapterError,
    RuntimeError, RuntimeErrorCode, Terminality,
};
use serde::Serialize;
use serde::Deserialize;

/// The wire's shared state: the two trait halves of the one config-selected
/// transport (D33) — Api ops route through `api`, the replication channels and
/// op-lifecycle through `link`.
#[derive(Clone)]
struct LinkState {
    api: Arc<dyn AuthorityServerApi>,
    link: Arc<dyn AuthorityServerLink>,
}

// ---- server-side deadlines (RFC-L2-lifecycle-and-errors D64 / M24) --------
//
// This wire has the same gap the `/v1` boundary had (audit N10): every
// up-channel handler and the down-channel's SETUP await were unbounded, with
// no deadline anywhere in this file. Same treatment, mirrored from
// `posthaste-http-api-adapter`'s `router.rs`/`deadlines.rs`: a blanket
// `TimeoutLayer` over the regular (non-stream) routes, plus an explicit
// deadline on the down-channel's own subscribe-setup call — the streaming
// phase itself stays unbounded (keepalive/idle-reap is a separate concern).
//
// The values are **not** shared with the adapter's constants (different
// crate, no shared dependency for this narrow purpose) but are chosen to
// match: 30s for a regular up-channel call, 10s for the down-channel's
// in-process subscribe-setup — review-flagged defaults, not measurements.
const LINK_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const LINK_SUBSCRIBE_SETUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Internal sentinel status `TimeoutLayer` emits when [`LINK_REQUEST_TIMEOUT`]
/// elapses (`tower_http::timeout::TimeoutLayer` ≥0.6.7 returns a bare
/// empty-body response at this status directly, never a `BoxError` — no
/// `HandleErrorLayer` needed). Not otherwise returned by any regular link
/// handler, so [`rewrite_link_timeout_response`] recognizes it unambiguously.
const LINK_TIMEOUT_SENTINEL_STATUS: StatusCode = StatusCode::GATEWAY_TIMEOUT;

/// Rewrite the bare [`LINK_TIMEOUT_SENTINEL_STATUS`] response into the same
/// typed [`LinkError`] envelope every other link-wire error uses.
async fn rewrite_link_timeout_response(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    if response.status() == LINK_TIMEOUT_SENTINEL_STATUS {
        LinkError::timeout().into_response()
    } else {
        response
    }
}

/// Apply the blanket per-request deadline (D64) to the link wire's regular
/// (non-stream) routes.
fn with_link_request_timeout(router: Router<LinkState>) -> Router<LinkState> {
    router
        .layer(TimeoutLayer::with_status_code(
            LINK_TIMEOUT_SENTINEL_STATUS,
            LINK_REQUEST_TIMEOUT,
        ))
        .layer(from_fn(rewrite_link_timeout_response))
}

/// Authentication + identity policy for the runtime↔authority-server link surface.
///
/// The link is a peer/infrastructure boundary (a split runtime authenticating
/// to its authority server), not the end-user capability surface the `/v1` macaroons
/// govern, so it authenticates runtimes with bearer tokens rather than
/// attenuable macaroons. There is no single-runtime special case: every
/// connecting runtime presents its own token, and the middleware resolves it to
/// a [`AuthorityServerLinkId`] (X runtimes, X ≥ 1) so the authority server can scope mutation
/// idempotency and confirmation per runtime
/// ([replication authority-server-link L1 §3.1](../replication/authority-server-link/L1.md)).
/// [`PerRuntime`](LinkAuth::PerRuntime) carries the `token → AuthorityServerLinkId` map and
/// constant-time compares the presented token against each key; the resolved id
/// is threaded into the up-channel handler. [`Disabled`](LinkAuth::Disabled) is
/// the in-process/test default (no token; a single anonymous runtime id) — a
/// remote mount MUST use `PerRuntime` (the link is otherwise unauthenticated).
pub enum LinkAuth {
    Disabled,
    PerRuntime(HashMap<String, AuthorityServerLinkId>),
}

impl LinkAuth {
    /// Resolve the link auth from daemon settings, **fail-closed**.
    ///
    /// Under `require_auth`, `[link].runtimes` (`token → runtime_id`, X ≥ 1) is
    /// mandatory — absent or empty ⇒ panic (the authority server is network-exposed; a
    /// misconfigured serve must not come up unauthenticated). With auth off
    /// (explicit dev opt-out) the link is unauthenticated (`Disabled`).
    ///
    /// `role` labels the panic message so each mounting binary reports its own
    /// context (e.g. `"posthaste-authority-server"`, `"[link] serve"`). The fail-closed
    /// condition (panic on `require_auth` with no `[link].runtimes`) is identical
    /// at every call site.
    pub fn from_daemon_settings(daemon: &DaemonSettings, role: &str) -> LinkAuth {
        if daemon.require_auth {
            match &daemon.link_runtimes {
                Some(map) if !map.is_empty() => LinkAuth::PerRuntime(
                    map.iter()
                        .map(|(token, rid)| (token.clone(), AuthorityServerLinkId::new(rid.clone())))
                        .collect(),
                ),
                _ => panic!(
                    "{role} requires [link].runtimes (token → runtime_id) under require_auth — \
                     one entry per connecting runtime (X ≥ 1)"
                ),
            }
        } else {
            LinkAuth::Disabled
        }
    }
}

/// The link wire's own error type. The body shape mirrors the `/v1` error
/// envelope (`{ code, message, details }`, camelCase) so the remote runtime
/// client (`RemoteAuthorityServer`) sees the same response it does across the
/// in-process hop — but the vocabulary is local and narrow (the wire's errors
/// are its own, not the `/v1` platform's).
///
/// `code` is the snake_case wire string (matching `ApiErrorCode`'s serialization
/// for the runtime-error codes), so the JSON a remote runtime decodes today is
/// unchanged.
pub(crate) struct LinkError {
    status: StatusCode,
    body: LinkErrorBody,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LinkErrorBody {
    code: &'static str,
    message: String,
    /// The typed retryability verdict, carried across the remote hop so a
    /// far-node transient outage (e.g. a 503 mid-move) is not stringified into a
    /// terminal rejection on the near side (audit top-10 #1). The remote runtime
    /// client (`RemoteAuthorityServer::post_link`) reads it back.
    terminality: Terminality,
    details: serde_json::Value,
}

impl LinkError {
    /// Map a runtime error to the link-wire error envelope.
    pub(crate) fn from_runtime_error(error: RuntimeError) -> Self {
        let RuntimeAdapterError {
            code,
            message,
            terminality,
            details,
            ..
        } = error.envelope();
        let (status, wire_code) = runtime_error_status(code);
        Self {
            status,
            body: LinkErrorBody {
                code: wire_code,
                message: message.clone(),
                terminality: *terminality,
                details: details.clone(),
            },
        }
    }

    /// 401: the presented bearer token is missing or unknown.
    pub(crate) fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            body: LinkErrorBody {
                code: "unauthorized",
                message: "missing or invalid bearer token".to_string(),
                terminality: Terminality::Permanent,
                details: serde_json::json!({}),
            },
        }
    }

    /// 503: a server-side deadline elapsed waiting on a runtime call (D64) —
    /// either the blanket regular-route `TimeoutLayer` or the down-channel's
    /// subscribe-setup deadline. Transient (M29 vocabulary): a retry after
    /// backoff may succeed. Mirrors `RuntimeErrorCode::ProviderUnavailable`'s
    /// existing `gateway_unavailable`/503 pairing rather than inventing a new
    /// code.
    pub(crate) fn timeout() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            body: LinkErrorBody {
                code: "gateway_unavailable",
                message: "request exceeded its deadline".to_string(),
                terminality: Terminality::Transient,
                details: serde_json::json!({}),
            },
        }
    }
}

impl IntoResponse for LinkError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

/// Map a runtime error code to its HTTP status + snake_case wire code. Mirrors
/// the `/v1` `ApiError` mapping so the link wire returns byte-identical responses
/// (the wire's errors are its own, but the remote runtime client decodes the same
/// envelope).
fn runtime_error_status(code: &RuntimeErrorCode) -> (StatusCode, &'static str) {
    match code {
        RuntimeErrorCode::RuntimeNotReady => (StatusCode::SERVICE_UNAVAILABLE, "internal_error"),
        RuntimeErrorCode::InvalidDescriptor | RuntimeErrorCode::InvalidMutation => {
            (StatusCode::BAD_REQUEST, "invalid_query")
        }
        RuntimeErrorCode::InvalidSecret => (StatusCode::BAD_REQUEST, "invalid_secret"),
        RuntimeErrorCode::InvalidAccount => (StatusCode::BAD_REQUEST, "invalid_account"),
        RuntimeErrorCode::AccountBaseUrlRequired => {
            (StatusCode::BAD_REQUEST, "account_base_url_required")
        }
        RuntimeErrorCode::AccountSecretRequired => (StatusCode::BAD_REQUEST, "account_secret_required"),
        RuntimeErrorCode::AccountUsernameRequired => {
            (StatusCode::BAD_REQUEST, "account_username_required")
        }
        RuntimeErrorCode::AccountSenderRequired => (StatusCode::BAD_REQUEST, "account_sender_required"),
        RuntimeErrorCode::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
        RuntimeErrorCode::NotFound => (StatusCode::NOT_FOUND, "not_found"),
        RuntimeErrorCode::ProviderUnavailable => {
            (StatusCode::SERVICE_UNAVAILABLE, "gateway_unavailable")
        }
        RuntimeErrorCode::Conflict => (StatusCode::CONFLICT, "conflict"),
        RuntimeErrorCode::NetworkError => (StatusCode::BAD_GATEWAY, "network_error"),
        RuntimeErrorCode::StateMismatch => (StatusCode::CONFLICT, "state_mismatch"),
        RuntimeErrorCode::CannotCalculateChanges => {
            (StatusCode::INTERNAL_SERVER_ERROR, "cannot_calculate_changes")
        }
        RuntimeErrorCode::GatewayRejected => (StatusCode::BAD_REQUEST, "gateway_rejected"),
        RuntimeErrorCode::SecretUnavailable => (StatusCode::BAD_REQUEST, "secret_unavailable"),
        RuntimeErrorCode::SecretUnsupported => (StatusCode::BAD_REQUEST, "secret_unsupported"),
        RuntimeErrorCode::StorageFailure => (StatusCode::INTERNAL_SERVER_ERROR, "storage_failure"),
        RuntimeErrorCode::StorageCorrupted => {
            (StatusCode::INTERNAL_SERVER_ERROR, "storage_corrupted")
        }
        RuntimeErrorCode::ConfigValidation => (StatusCode::BAD_REQUEST, "config_validation"),
        RuntimeErrorCode::ConfigIo => (StatusCode::INTERNAL_SERVER_ERROR, "config_io"),
        RuntimeErrorCode::ConfigParse => (StatusCode::BAD_REQUEST, "config_parse"),
        RuntimeErrorCode::TransportDisconnected | RuntimeErrorCode::Internal => {
            (StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        }
    }
}

/// Constant-time byte equality. Compares the full length of both inputs without
/// early-exit on the first mismatch, so timing does not leak how many leading
/// bytes matched. Differing lengths short-circuit to `false` (length is not
/// itself secret), but equal-length inputs are compared in full.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Extract the bearer token from an `Authorization: Bearer <token>` header.
fn bearer_token(req: &Request) -> Option<&str> {
    let value = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    let rest = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    Some(rest.trim())
}

/// Middleware (`PerRuntime`): resolve the presented bearer token to a
/// [`AuthorityServerLinkId`] via the `token → AuthorityServerLinkId` map, constant-time comparing the
/// presented token against each key, then thread the id into the request
/// extensions for the up-channel handler. An unknown token (or none) is a 401.
async fn require_link_token(
    State(tokens): State<Arc<HashMap<String, AuthorityServerLinkId>>>,
    mut req: Request,
    next: Next,
) -> Response {
    let presented = bearer_token(&req);
    let runtime_id = presented.and_then(|p| {
        tokens
            .iter()
            .find_map(|(t, id)| constant_time_eq(p.as_bytes(), t.as_bytes()).then(|| id.clone()))
    });
    match runtime_id {
        Some(id) => {
            req.extensions_mut().insert(id);
            next.run(req).await
        }
        _ => LinkError::unauthorized().into_response(),
    }
}

/// Middleware (`Disabled`): thread a single anonymous [`AuthorityServerLinkId`] so the
/// up-channel handler's `Extension<AuthorityServerLinkId>` is satisfied without auth. The
/// dev/test default — a remote mount MUST use `PerRuntime`.
async fn inject_runtime_id(
    State(runtime_id): State<AuthorityServerLinkId>,
    mut req: Request,
    next: Next,
) -> Response {
    req.extensions_mut().insert(runtime_id);
    next.run(req).await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubscribeQuery {
    /// JSON-encoded [`LinkCoverage`]; absent means complete coverage.
    coverage: Option<String>,
    /// The resume cursor (D46): resume the down-stream from just after this seq.
    /// Absent opens a fresh stream. Coverage says WHAT to stream, `after_seq`
    /// says WHERE to resume.
    after_seq: Option<u64>,
}

/// Up-channel: apply a forwarded named mutation and return the authority server's receipt.
async fn forward_mutation(
    State(state): State<LinkState>,
    Extension(runtime_id): Extension<AuthorityServerLinkId>,
    Json(request): Json<MutationRequest>,
) -> Result<Json<MutationReceipt>, LinkError> {
    state
        .link
        .forward_mutation_for(&runtime_id, request)
        .await
        .map(Json)
        .map_err(LinkError::from_runtime_error)
}

/// Down-channel: stream authoritative base-assertion frames as SSE.
async fn subscribe(
    State(state): State<LinkState>,
    Extension(runtime_id): Extension<AuthorityServerLinkId>,
    Query(query): Query<SubscribeQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, LinkError> {
    let coverage = query
        .coverage
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or(LinkCoverage::Complete);
    // D64/M24: this route is excluded from the blanket regular-route
    // `TimeoutLayer` (a stream is supposed to live long), so the SETUP await
    // — the runtime call that produces the subscription — takes its own
    // explicit deadline instead. The streaming phase after this point is
    // unbounded.
    let stream = match tokio::time::timeout(
        LINK_SUBSCRIBE_SETUP_TIMEOUT,
        state
            .link
            .subscribe_for(&runtime_id, coverage, query.after_seq),
    )
    .await
    {
        Ok(result) => result.map_err(LinkError::from_runtime_error)?,
        Err(_) => return Err(LinkError::timeout()),
    };
    Ok(Sse::new(stream.map(down_frame_to_sse)).keep_alive(KeepAlive::default()))
}

/// Read channel: compute a mail-list query at the far node.
async fn query_mail_page(
    State(state): State<LinkState>,
    Json(request): Json<MailQueryRequest>,
) -> Result<Json<MailQueryPage>, LinkError> {
    state
        .api
        .query_mail_page(request)
        .await
        .map(Json)
        .map_err(LinkError::from_runtime_error)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SummaryRequest {
    account_id: AccountId,
    message_id: MessageId,
}

/// Read channel: the current summary of one message (the point read).
async fn current_summary(
    State(state): State<LinkState>,
    Json(request): Json<SummaryRequest>,
) -> Result<Json<Option<MessageSummary>>, LinkError> {
    state
        .api
        .current_summary(request.account_id, request.message_id)
        .await
        .map(Json)
        .map_err(LinkError::from_runtime_error)
}

/// Read channel: a message's detail (the `messageDetail` view).
async fn message_detail(
    State(state): State<LinkState>,
    Json(request): Json<SummaryRequest>,
) -> Result<Json<Option<MessageDetail>>, LinkError> {
    state
        .api
        .message_detail(request.account_id, request.message_id)
        .await
        .map(Json)
        .map_err(LinkError::from_runtime_error)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversationRequest {
    conversation_id: ConversationId,
}

/// Read channel: an overlay-folded conversation (the `conversation` view).
async fn conversation(
    State(state): State<LinkState>,
    Json(request): Json<ConversationRequest>,
) -> Result<Json<ConversationView>, LinkError> {
    state
        .api
        .conversation(request.conversation_id)
        .await
        .map(Json)
        .map_err(LinkError::from_runtime_error)
}

/// Emit a far-node handler per Api-op row + the function that registers them
/// all. Generated from the shared Api-op table so the server surface cannot
/// drift from the `RemoteAuthorityServer` client.
macro_rules! emit_link_api_routes {
    ($($method:ident => $path:literal => $req:ident { $($field:ident : $fty:ty),* $(,)? } => $ret:ty;)*) => {
        $(
            // Zero-arg ops leave `req` unused (the request body is `{}`).
            #[allow(unused_variables)]
            async fn $method(
                State(state): State<LinkState>,
                Json(req): Json<posthaste_authority_server_link::$req>,
            ) -> Result<Json<$ret>, LinkError> {
                let result: Result<$ret, RuntimeError> =
                    state.api.$method($(req.$field),*).await;
                result.map(Json).map_err(LinkError::from_runtime_error)
            }
        )*

        fn register_generated_api_routes(router: Router<LinkState>) -> Router<LinkState> {
            router $( .route($path, post($method)) )*
        }
    };
}
posthaste_authority_server_link::for_each_link_api_op!(emit_link_api_routes);

/// Emit a far-node handler per op-lifecycle row (the Link half's outbox
/// lifecycle mutations) + the function that registers them.
macro_rules! emit_link_lifecycle_routes {
    ($($method:ident => $path:literal => $req:ident { $($field:ident : $fty:ty),* $(,)? } => $ret:ty;)*) => {
        $(
            async fn $method(
                State(state): State<LinkState>,
                Json(req): Json<posthaste_authority_server_link::$req>,
            ) -> Result<Json<$ret>, LinkError> {
                let result: Result<$ret, RuntimeError> =
                    state.link.$method($(req.$field),*).await;
                result.map(Json).map_err(LinkError::from_runtime_error)
            }
        )*

        fn register_generated_lifecycle_routes(router: Router<LinkState>) -> Router<LinkState> {
            router $( .route($path, post($method)) )*
        }
    };
}
posthaste_authority_server_link::for_each_link_lifecycle_op!(emit_link_lifecycle_routes);

/// The five preserved message-command routes (M5b): each decodes its pre-split
/// request struct, rebuilds the typed `MailOperation`, and routes through the
/// single [`AuthorityServerApi::apply`] entry — one command dispatch per
/// implementor, not one per route. Paths and JSON are wire-identical to the
/// pre-split per-command RPCs.
macro_rules! emit_link_command_routes {
    ($(($handler:ident, $req:ident, $path:ident);)*) => {
        $(
            async fn $handler(
                State(state): State<LinkState>,
                Json(req): Json<$req>,
            ) -> Result<Json<CommandAck>, LinkError> {
                state
                    .api
                    .apply(req.into_operation())
                    .await
                    .map(Json)
                    .map_err(LinkError::from_runtime_error)
            }
        )*

        fn register_command_routes(router: Router<LinkState>) -> Router<LinkState> {
            router $( .route($path, post($handler)) )*
        }
    };
}
emit_link_command_routes! {
    (set_keywords_command, SetKeywordsRequest, LINK_SET_KEYWORDS_PATH);
    (add_to_mailbox_command, AddToMailboxRequest, LINK_ADD_TO_MAILBOX_PATH);
    (remove_from_mailbox_command, RemoveFromMailboxRequest, LINK_REMOVE_FROM_MAILBOX_PATH);
    (replace_mailboxes_command, ReplaceMailboxesRequest, LINK_REPLACE_MAILBOXES_PATH);
    (destroy_message_command, DestroyMessageRequest, LINK_DESTROY_MESSAGE_PATH);
}

fn down_frame_to_sse(frame: SequencedFrame) -> Result<Event, Infallible> {
    Ok(Event::default()
        .json_data(frame)
        .unwrap_or_else(|_| Event::default().data("{}")))
}

/// Build the far-node link router over a transport pair — the authority server's
/// in-process `AuthorityServerLinkHandle` in a split deployment. The handle
/// carries both trait halves of the D33 seam; the wire serves both over the
/// existing routes (routes/wire unchanged by the split).
pub fn link_router(transport: AuthorityServerLinkHandle, auth: LinkAuth) -> Router {
    let state = LinkState {
        api: transport.api().clone(),
        link: transport.link().clone(),
    };

    // D64/M24: the down-channel SSE stream is split into its own sub-router so
    // the blanket `TimeoutLayer` below — applied only to `regular_routes` —
    // cannot wrongly cut a long-lived stream; `subscribe` deadline-wraps its
    // own SETUP await instead (see the const block above).
    let stream_routes = Router::new().route(LINK_SUBSCRIBE_PATH, get(subscribe));

    let regular_routes = Router::new()
        .route(LINK_FORWARD_MUTATION_PATH, post(forward_mutation))
        .route(LINK_QUERY_PATH, post(query_mail_page))
        .route(LINK_SUMMARY_PATH, post(current_summary))
        .route(LINK_DETAIL_PATH, post(message_detail))
        .route(LINK_CONVERSATION_PATH, post(conversation));
    // The full request/response surface (reads + typed writes + the preserved
    // message-command routes + the op-lifecycle) is generated from the shared
    // link-op tables.
    let regular_routes = register_command_routes(register_generated_lifecycle_routes(
        register_generated_api_routes(regular_routes),
    ));
    let regular_routes = with_link_request_timeout(regular_routes);

    let router = regular_routes.merge(stream_routes).with_state(state);
    match auth {
        LinkAuth::Disabled => router.layer(from_fn_with_state(
            AuthorityServerLinkId::new(uuid::Uuid::new_v4().to_string()),
            inject_runtime_id,
        )),
        LinkAuth::PerRuntime(tokens) => {
            router.layer(from_fn_with_state(Arc::new(tokens), require_link_token))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use posthaste_authority_server_link::{AuthorityServerLinkHandle, DownStream};
    use tower::ServiceExt;

    /// A far-node transport whose read-channel `current_summary` (a regular
    /// up-channel route) and `subscribe` (the down-channel's SETUP call) never
    /// resolve — the wedge-prone case D64/M24 exists to fix.
    struct HangingFarNode;

    #[async_trait]
    impl AuthorityServerApi for HangingFarNode {
        async fn current_summary(
            &self,
            _account_id: AccountId,
            _message_id: MessageId,
        ) -> Result<Option<MessageSummary>, RuntimeError> {
            std::future::pending().await
        }
    }

    #[async_trait]
    impl AuthorityServerLink for HangingFarNode {
        async fn forward_mutation(
            &self,
            _mutation: MutationRequest,
        ) -> Result<MutationReceipt, RuntimeError> {
            std::future::pending().await
        }

        async fn subscribe(
            &self,
            _coverage: LinkCoverage,
            _after_seq: Option<u64>,
        ) -> Result<DownStream, RuntimeError> {
            std::future::pending().await
        }
    }

    fn hanging_router() -> Router {
        link_router(
            AuthorityServerLinkHandle::new(Arc::new(HangingFarNode)),
            LinkAuth::Disabled,
        )
    }

    /// M24 gate: a wedged runtime call behind a regular (non-stream) link-wire
    /// route must return a timeout, not hang the up-channel handler forever —
    /// the same gap the `/v1` boundary had (audit N10), mirrored here.
    #[tokio::test(start_paused = true)]
    async fn a_wedged_up_channel_call_times_out_instead_of_hanging() {
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(LINK_SUMMARY_PATH)
            .header(header::CONTENT_TYPE, "application/json")
            .body(axum::body::Body::from(
                serde_json::json!({ "accountId": "acct-1", "messageId": "m-1" }).to_string(),
            ))
            .unwrap();
        let response = hanging_router()
            .oneshot(request)
            .await
            .expect("service must not error");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    /// M24 gate: a wedged down-channel SETUP await (the `subscribe_for` call
    /// that produces the SSE stream) must return a timeout, not hang — the
    /// streaming route is excluded from the blanket layer, so this proves the
    /// per-await deadline inside `subscribe` itself is load-bearing.
    #[tokio::test(start_paused = true)]
    async fn a_wedged_down_channel_setup_await_times_out_instead_of_hanging() {
        let request = axum::http::Request::builder()
            .method("GET")
            .uri(LINK_SUBSCRIBE_PATH)
            .body(axum::body::Body::empty())
            .unwrap();
        let response = hanging_router()
            .oneshot(request)
            .await
            .expect("service must not error");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
