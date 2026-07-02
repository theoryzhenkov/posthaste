use super::*;

#[utoipa::path(
    post,
    path = "/v1/runtime/sessions",
    tag = "runtime",
    summary = "Open a runtime link",
    description = "Creates a runtime link whose stream carries RuntimeFrame values.",
    params(RuntimeLinkQuery),
    responses(
        (status = 200, description = "The opened runtime link", body = RuntimeLinkConnection),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn open_runtime_link(
    State(state): State<Arc<AppState>>,
    Query(query): Query<RuntimeLinkQuery>,
) -> Result<Json<RuntimeLinkConnection>, ApiError> {
    let mut caller = runtime_caller(query.source_id.as_deref());
    caller.capabilities.view_delta = query.view_delta.unwrap_or(false);
    state
        .runtime
        .open_link(caller)
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

#[utoipa::path(
    delete,
    path = "/v1/runtime/sessions/{session_id}",
    tag = "runtime",
    summary = "Close a runtime link",
    description = "Closes a runtime link and releases its open runtime views.",
    params(
        ("session_id" = String, Path, description = "Runtime link id"),
        RuntimeLinkQuery
    ),
    responses(
        (status = 200, description = "The link was closed", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime link", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn close_runtime_link(
    State(state): State<Arc<AppState>>,
    Path(link_id): Path<String>,
    Query(query): Query<RuntimeLinkQuery>,
) -> Result<Json<OkResponse>, ApiError> {
    state
        .runtime
        .close_link(
            runtime_caller(query.source_id.as_deref()),
            RuntimeLinkId::new(link_id),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OkResponse { ok: true }))
}

#[utoipa::path(
    get,
    path = "/v1/runtime/sessions/{session_id}/stream",
    tag = "runtime",
    summary = "Subscribe to runtime frames",
    description = "Streams link-scoped RuntimeFrame values as server-sent events. Event ids are linkSeq values.",
    params(
        ("session_id" = String, Path, description = "Runtime link id"),
        RuntimeLinkStreamQuery
    ),
    responses(
        (status = 200, description = "SSE stream of RuntimeFrame values"),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime link", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn stream_runtime_link(
    State(state): State<Arc<AppState>>,
    Path(link_id): Path<String>,
    Query(query): Query<RuntimeLinkStreamQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let subscription = state
        .runtime
        .subscribe_runtime_frames(
            runtime_caller(query.source_id.as_deref()),
            RuntimeLinkId::new(link_id),
            query.after_seq.map(RuntimeLinkSeq::new),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    let catch_up_stream = tokio_stream::iter(subscription.catch_up.into_iter().map(frame_to_sse));
    let live_stream = subscription.live.map(frame_to_sse);
    Ok(Sse::new(catch_up_stream.chain(live_stream)).keep_alive(KeepAlive::default()))
}
