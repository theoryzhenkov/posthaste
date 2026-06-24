//! The backend far-node HTTP surface: serves the runtime↔backend link wire.
//!
//! The symmetric inner twin of the client↔runtime API. Where that surface lets
//! a remote client drive the runtime, this one lets a **remote runtime** drive
//! the **backend** ([replication L4 §4](../replication/L4.md)): the up-channel is
//! a `POST` of a named mutation, the down-channel an SSE stream of base-assertion
//! frames. A split-backend host mounts this over the backend's in-process
//! `BackendLink` transport (`AuthorityRuntimeBuild::backend_link`); a runtime
//! configured with `transport = "remote"` connects to it via `RemoteTransport`.
//!
//! The wire paths and frame types are the shared contract
//! ([`posthaste_link_contract`]) — one definition, both ends — so client and
//! server cannot drift (assertion `one-link-transport`).
//!
//! @spec docs/replication/L4#4-the-transport-abstraction-one-seam-for-both-links

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use posthaste_domain::{
    AccountId, ConversationId, ConversationView, MessageDetail, MessageId, MessageSummary,
};
use posthaste_link_contract::{
    BackendApi, DownFrame, LinkCoverage, LINK_CONVERSATION_PATH, LINK_DETAIL_PATH,
    LINK_FORWARD_MUTATION_PATH, LINK_QUERY_PATH, LINK_SUBSCRIBE_PATH, LINK_SUMMARY_PATH,
};
use posthaste_runtime_contract::{MailQueryPage, MailQueryRequest, MutationReceipt, MutationRequest};
use serde::Deserialize;

use crate::api::ApiError;

#[derive(Clone)]
struct LinkState {
    transport: Arc<dyn BackendApi>,
}

#[derive(Debug, Deserialize)]
struct SubscribeQuery {
    /// JSON-encoded [`LinkCoverage`]; absent means complete coverage.
    coverage: Option<String>,
}

/// Up-channel: apply a forwarded named mutation and return the backend's receipt.
async fn forward_mutation(
    State(state): State<LinkState>,
    Json(request): Json<MutationRequest>,
) -> Result<Json<MutationReceipt>, ApiError> {
    state
        .transport
        .forward_mutation(request)
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// Down-channel: stream authoritative base-assertion frames as SSE.
async fn subscribe(
    State(state): State<LinkState>,
    Query(query): Query<SubscribeQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let coverage = query
        .coverage
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or(LinkCoverage::Complete);
    let stream = state
        .transport
        .subscribe(coverage)
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

fn down_frame_to_sse(frame: DownFrame) -> Result<Event, Infallible> {
    Ok(Event::default()
        .json_data(frame)
        .unwrap_or_else(|_| Event::default().data("{}")))
}

/// Build the far-node link router over a transport — the backend's in-process
/// `BackendLink` transport in a split deployment.
pub fn link_router(transport: Arc<dyn BackendApi>) -> Router {
    Router::new()
        .route(LINK_FORWARD_MUTATION_PATH, post(forward_mutation))
        .route(LINK_SUBSCRIBE_PATH, get(subscribe))
        .route(LINK_QUERY_PATH, post(query_mail_page))
        .route(LINK_SUMMARY_PATH, post(current_summary))
        .route(LINK_DETAIL_PATH, post(message_detail))
        .route(LINK_CONVERSATION_PATH, post(conversation))
        .with_state(LinkState { transport })
}
