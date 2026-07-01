//! The backend far-node HTTP surface: serves the runtime↔backend link wire.
//!
//! The symmetric inner twin of the client↔runtime API. Where that surface lets
//! a remote client drive the runtime, this one lets a **remote runtime** drive
//! the **backend** ([replication backend-link L2 §3](../replication/backend-link/L2.md)): the up-channel is
//! a `POST` of a named mutation, the down-channel an SSE stream of base-assertion
//! frames. A split-backend host mounts this over the backend's in-process
//! `BackendLink` transport (`AuthorityRuntimeBuild::backend_link`); a runtime
//! configured with `transport = "remote"` connects to it via `RemoteTransport`.
//!
//! The wire paths and frame types are the shared contract
//! ([`posthaste_link_contract`]) — one definition, both ends — so client and
//! server cannot drift (assertion `one-link-transport`).
//!
//! @spec docs/replication/backend-link/L2#3-the-link-wire-link_router

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Extension, Query, Request, State};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use posthaste_domain::{
    AccountId, ConversationId, ConversationView, MessageDetail, MessageId, MessageSummary,
};
use posthaste_link_contract::{
    BackendApi, DownFrame, LinkCoverage, RuntimeId, LINK_CONVERSATION_PATH, LINK_DETAIL_PATH,
    LINK_FORWARD_MUTATION_PATH, LINK_QUERY_PATH, LINK_SUBSCRIBE_PATH, LINK_SUMMARY_PATH,
};
use posthaste_runtime_contract::{
    MailQueryPage, MailQueryRequest, MutationReceipt, MutationRequest,
};
use serde::Deserialize;

use crate::api::ApiError;
use crate::auth::{bearer_token, constant_time_eq, unauthorized};

#[derive(Clone)]
struct LinkState {
    transport: Arc<dyn BackendApi>,
}

/// Authentication + identity policy for the runtime↔backend link surface.
///
/// The link is a peer/infrastructure boundary (a split runtime authenticating
/// to its backend), not the end-user capability surface the `/v1` macaroons
/// govern, so it authenticates runtimes with bearer tokens rather than
/// attenuable macaroons. There is no single-runtime special case: every
/// connecting runtime presents its own token, and the middleware resolves it to
/// a [`RuntimeId`] (X runtimes, X ≥ 1) so the backend can scope mutation
/// idempotency and confirmation per runtime
/// ([replication backend-link L1 §3.1](../replication/backend-link/L1.md)).
/// [`PerRuntime`](LinkAuth::PerRuntime) carries the `token → RuntimeId` map and
/// constant-time compares the presented token against each key; the resolved id
/// is threaded into the up-channel handler. [`Disabled`](LinkAuth::Disabled) is
/// the in-process/test default (no token; a single anonymous runtime id) — a
/// remote mount MUST use `PerRuntime` (the link is otherwise unauthenticated).
pub enum LinkAuth {
    Disabled,
    PerRuntime(HashMap<String, RuntimeId>),
}

/// Middleware (`PerRuntime`): resolve the presented bearer token to a
/// [`RuntimeId`] via the `token → RuntimeId` map, constant-time comparing the
/// presented token against each key, then thread the id into the request
/// extensions for the up-channel handler. An unknown token (or none) is a 401.
async fn require_link_token(
    State(tokens): State<Arc<HashMap<String, RuntimeId>>>,
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
        _ => unauthorized().into_response(),
    }
}

/// Middleware (`Disabled`): thread a single anonymous [`RuntimeId`] so the
/// up-channel handler's `Extension<RuntimeId>` is satisfied without auth. The
/// dev/test default — a remote mount MUST use `PerRuntime`.
async fn inject_runtime_id(
    State(runtime_id): State<RuntimeId>,
    mut req: Request,
    next: Next,
) -> Response {
    req.extensions_mut().insert(runtime_id);
    next.run(req).await
}

#[derive(Debug, Deserialize)]
struct SubscribeQuery {
    /// JSON-encoded [`LinkCoverage`]; absent means complete coverage.
    coverage: Option<String>,
}

/// Up-channel: apply a forwarded named mutation and return the backend's receipt.
async fn forward_mutation(
    State(state): State<LinkState>,
    Extension(runtime_id): Extension<RuntimeId>,
    Json(request): Json<MutationRequest>,
) -> Result<Json<MutationReceipt>, ApiError> {
    state
        .transport
        .forward_mutation_for(&runtime_id, request)
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// Down-channel: stream authoritative base-assertion frames as SSE.
async fn subscribe(
    State(state): State<LinkState>,
    Extension(runtime_id): Extension<RuntimeId>,
    Query(query): Query<SubscribeQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let coverage = query
        .coverage
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or(LinkCoverage::Complete);
    let stream = state
        .transport
        .subscribe_for(&runtime_id, coverage)
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Sse::new(stream.map(down_frame_to_sse)).keep_alive(KeepAlive::default()))
}

/// Read channel: compute a mail-list query at the far node.
async fn query_mail_page(
    State(state): State<LinkState>,
    Json(request): Json<MailQueryRequest>,
) -> Result<Json<MailQueryPage>, ApiError> {
    state
        .transport
        .query_mail_page(request)
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
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
) -> Result<Json<Option<MessageSummary>>, ApiError> {
    state
        .transport
        .current_summary(request.account_id, request.message_id)
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// Read channel: a message's detail (the `messageDetail` view).
async fn message_detail(
    State(state): State<LinkState>,
    Json(request): Json<SummaryRequest>,
) -> Result<Json<Option<MessageDetail>>, ApiError> {
    state
        .transport
        .message_detail(request.account_id, request.message_id)
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
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
) -> Result<Json<ConversationView>, ApiError> {
    state
        .transport
        .conversation(request.conversation_id)
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// Emit a far-node handler per link-op row + the function that registers them
/// all. Generated from the shared link-op table so the server surface cannot
/// drift from the [`RemoteBackend`](posthaste_authority_runtime) client.
macro_rules! emit_link_routes {
    ($($method:ident => $path:literal => $req:ident { $($field:ident : $fty:ty),* $(,)? } => $ret:ty;)*) => {
        $(
            // Zero-arg ops leave `req` unused (the request body is `{}`).
            #[allow(unused_variables)]
            async fn $method(
                State(state): State<LinkState>,
                Json(req): Json<posthaste_link_contract::$req>,
            ) -> Result<Json<$ret>, ApiError> {
                let result: Result<$ret, posthaste_runtime_contract::RuntimeError> =
                    state.transport.$method($(req.$field),*).await;
                result.map(Json).map_err(ApiError::from_runtime_error)
            }
        )*

        fn register_generated_link_routes(router: Router<LinkState>) -> Router<LinkState> {
            router $( .route($path, post($method)) )*
        }
    };
}
posthaste_link_contract::for_each_link_op!(emit_link_routes);

fn down_frame_to_sse(frame: DownFrame) -> Result<Event, Infallible> {
    Ok(Event::default()
        .json_data(frame)
        .unwrap_or_else(|_| Event::default().data("{}")))
}

/// Build the far-node link router over a transport — the backend's in-process
/// `BackendLink` transport in a split deployment.
pub fn link_router(transport: Arc<dyn BackendApi>, auth: LinkAuth) -> Router {
    let router = Router::new()
        .route(LINK_FORWARD_MUTATION_PATH, post(forward_mutation))
        .route(LINK_SUBSCRIBE_PATH, get(subscribe))
        .route(LINK_QUERY_PATH, post(query_mail_page))
        .route(LINK_SUMMARY_PATH, post(current_summary))
        .route(LINK_DETAIL_PATH, post(message_detail))
        .route(LINK_CONVERSATION_PATH, post(conversation));
    // The full request/response surface (reads + typed writes) is generated
    // from the shared link-op table.
    let router = register_generated_link_routes(router).with_state(LinkState { transport });
    match auth {
        LinkAuth::Disabled => router.layer(from_fn_with_state(
            RuntimeId::new(uuid::Uuid::new_v4().to_string()),
            inject_runtime_id,
        )),
        LinkAuth::PerRuntime(tokens) => {
            router.layer(from_fn_with_state(Arc::new(tokens), require_link_token))
        }
    }
}
