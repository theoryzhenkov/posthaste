use super::*;
use posthaste_runtime_contract::{
    RuntimeFrame, RuntimeSession, RuntimeSessionId, RuntimeSessionSeq, ViewDescriptor, ViewId,
    ViewSnapshot,
};

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionQuery {
    pub source_id: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionStreamQuery {
    pub after_seq: Option<u64>,
    pub source_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenRuntimeSessionViewRequest {
    #[schema(value_type = Object)]
    pub descriptor: ViewDescriptor,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenRuntimeSessionViewResponse {
    #[schema(value_type = String)]
    pub view_id: ViewId,
    pub snapshot: ViewSnapshot,
}

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
    state
        .runtime
        .open_session(runtime_caller(query.source_id.as_deref()))
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

#[utoipa::path(
    post,
    path = "/v1/runtime/sessions/{session_id}/views",
    tag = "runtime",
    summary = "Open a runtime session view",
    description = "Opens a view in a runtime session and returns its initial snapshot.",
    params(
        ("session_id" = String, Path, description = "Runtime session id"),
        RuntimeSessionQuery
    ),
    request_body = OpenRuntimeSessionViewRequest,
    responses(
        (status = 200, description = "The opened view snapshot", body = OpenRuntimeSessionViewResponse),
        (status = 400, description = "Invalid view descriptor", body = ApiErrorBody),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime session", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn open_runtime_session_view(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Query(query): Query<RuntimeSessionQuery>,
    Json(request): Json<OpenRuntimeSessionViewRequest>,
) -> Result<Json<OpenRuntimeSessionViewResponse>, ApiError> {
    let snapshot = state
        .runtime
        .open_session_view(
            runtime_caller(query.source_id.as_deref()),
            RuntimeSessionId::new(session_id),
            request.descriptor,
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OpenRuntimeSessionViewResponse {
        view_id: snapshot.view_id.clone(),
        snapshot,
    }))
}

#[utoipa::path(
    delete,
    path = "/v1/runtime/sessions/{session_id}/views/{view_id}",
    tag = "runtime",
    summary = "Close a runtime session view",
    description = "Closes a runtime view for a session and emits a viewClosed RuntimeFrame.",
    params(
        ("session_id" = String, Path, description = "Runtime session id"),
        ("view_id" = String, Path, description = "Runtime view id"),
        RuntimeSessionQuery
    ),
    responses(
        (status = 200, description = "The view was closed", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime session", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn close_runtime_session_view(
    State(state): State<Arc<AppState>>,
    Path((session_id, view_id)): Path<(String, String)>,
    Query(query): Query<RuntimeSessionQuery>,
) -> Result<Json<OkResponse>, ApiError> {
    state
        .runtime
        .close_session_view(
            runtime_caller(query.source_id.as_deref()),
            RuntimeSessionId::new(session_id),
            ViewId::new(view_id),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OkResponse { ok: true }))
}

fn runtime_caller(source_id: Option<&str>) -> RuntimeCaller {
    let mut caller = RuntimeCaller::api();
    caller.account_scope = source_id.map(|source_id| vec![source_id.to_string()]);
    caller
}

fn frame_to_sse(frame: RuntimeFrame) -> Result<Event, Infallible> {
    Ok(Event::default()
        .id(frame.session_seq().get().to_string())
        .json_data(frame)
        .unwrap_or_else(|_| Event::default().data("{}")))
}
