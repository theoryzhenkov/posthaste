use super::*;

#[utoipa::path(
    post,
    path = "/v1/runtime/sessions",
    tag = "runtime",
    summary = "Open a runtime session",
    description = "Creates a runtime session whose stream carries RuntimeFrame values.",
    params(RuntimeSessionQuery),
    responses(
        (status = 200, description = "The opened runtime session", body = RuntimeSession),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn open_runtime_session(
    State(state): State<Arc<AppState>>,
    Query(query): Query<RuntimeSessionQuery>,
) -> Result<Json<RuntimeSession>, ApiError> {
    let mut caller = runtime_caller(query.source_id.as_deref());
    caller.capabilities.view_delta = query.view_delta.unwrap_or(false);
    state
        .runtime
        .open_session(caller)
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

#[utoipa::path(
    delete,
    path = "/v1/runtime/sessions/{session_id}",
    tag = "runtime",
    summary = "Close a runtime session",
    description = "Closes a runtime session and releases its open runtime views.",
    params(
        ("session_id" = String, Path, description = "Runtime session id"),
        RuntimeSessionQuery
    ),
    responses(
        (status = 200, description = "The session was closed", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime session", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn close_runtime_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Query(query): Query<RuntimeSessionQuery>,
) -> Result<Json<OkResponse>, ApiError> {
    state
        .runtime
        .close_session(
            runtime_caller(query.source_id.as_deref()),
            RuntimeSessionId::new(session_id),
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
    description = "Streams session-scoped RuntimeFrame values as server-sent events. Event ids are sessionSeq values.",
    params(
        ("session_id" = String, Path, description = "Runtime session id"),
        RuntimeSessionStreamQuery
    ),
    responses(
        (status = 200, description = "SSE stream of RuntimeFrame values"),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime session", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn stream_runtime_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Query(query): Query<RuntimeSessionStreamQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let subscription = state
        .runtime
        .subscribe_runtime_frames(
            runtime_caller(query.source_id.as_deref()),
            RuntimeSessionId::new(session_id),
            query.after_seq.map(RuntimeSessionSeq::new),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    let catch_up_stream = tokio_stream::iter(subscription.catch_up.into_iter().map(frame_to_sse));
    let live_stream = subscription.live.map(frame_to_sse);
    Ok(Sse::new(catch_up_stream.chain(live_stream)).keep_alive(KeepAlive::default()))
}
