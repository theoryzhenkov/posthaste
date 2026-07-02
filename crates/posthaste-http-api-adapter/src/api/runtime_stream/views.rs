use super::*;

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

#[utoipa::path(
    post,
    path = "/v1/runtime/sessions/{session_id}/views/{view_id}/extend",
    tag = "runtime",
    summary = "Extend a runtime session view window",
    description = "Grows an open windowed view (e.g. mailList) by the requested row count and returns the extended snapshot, also broadcast as a viewReplace RuntimeFrame.",
    params(
        ("session_id" = String, Path, description = "Runtime session id"),
        ("view_id" = String, Path, description = "Runtime view id"),
        RuntimeSessionQuery
    ),
    request_body = ExtendRuntimeSessionViewRequest,
    responses(
        (status = 200, description = "The extended view snapshot", body = OpenRuntimeSessionViewResponse),
        (status = 400, description = "View does not support window extension", body = ApiErrorBody),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime session or view", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn extend_runtime_session_view(
    State(state): State<Arc<AppState>>,
    Path((session_id, view_id)): Path<(String, String)>,
    Query(query): Query<RuntimeSessionQuery>,
    Json(request): Json<ExtendRuntimeSessionViewRequest>,
) -> Result<Json<OpenRuntimeSessionViewResponse>, ApiError> {
    let snapshot = state
        .runtime
        .extend_session_view(
            runtime_caller(query.source_id.as_deref()),
            RuntimeSessionId::new(session_id),
            ViewId::new(view_id),
            request.count,
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OpenRuntimeSessionViewResponse {
        view_id: snapshot.view_id.clone(),
        snapshot,
    }))
}
