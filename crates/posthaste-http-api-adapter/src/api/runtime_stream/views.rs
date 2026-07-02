use super::*;

#[utoipa::path(
    post,
    path = "/v1/runtime/sessions/{session_id}/views",
    tag = "runtime",
    summary = "Open a runtime link view",
    description = "Opens a view in a runtime link and returns its initial snapshot.",
    params(
        ("session_id" = String, Path, description = "Runtime link id"),
        RuntimeLinkQuery
    ),
    request_body = OpenRuntimeLinkViewRequest,
    responses(
        (status = 200, description = "The opened view snapshot", body = OpenRuntimeLinkViewResponse),
        (status = 400, description = "Invalid view descriptor", body = ApiErrorBody),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime link", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn open_runtime_link_view(
    State(state): State<Arc<AppState>>,
    Path(link_id): Path<String>,
    Query(query): Query<RuntimeLinkQuery>,
    Json(request): Json<OpenRuntimeLinkViewRequest>,
) -> Result<Json<OpenRuntimeLinkViewResponse>, ApiError> {
    let snapshot = state
        .runtime
        .open_link_view(
            runtime_caller(query.source_id.as_deref()),
            RuntimeLinkId::new(link_id),
            request.descriptor,
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OpenRuntimeLinkViewResponse {
        view_id: snapshot.view_id.clone(),
        snapshot,
    }))
}

#[utoipa::path(
    delete,
    path = "/v1/runtime/sessions/{session_id}/views/{view_id}",
    tag = "runtime",
    summary = "Close a runtime link view",
    description = "Closes a runtime view for a link and emits a viewClosed RuntimeFrame.",
    params(
        ("session_id" = String, Path, description = "Runtime link id"),
        ("view_id" = String, Path, description = "Runtime view id"),
        RuntimeLinkQuery
    ),
    responses(
        (status = 200, description = "The view was closed", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime link", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn close_runtime_link_view(
    State(state): State<Arc<AppState>>,
    Path((link_id, view_id)): Path<(String, String)>,
    Query(query): Query<RuntimeLinkQuery>,
) -> Result<Json<OkResponse>, ApiError> {
    state
        .runtime
        .close_link_view(
            runtime_caller(query.source_id.as_deref()),
            RuntimeLinkId::new(link_id),
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
    summary = "Extend a runtime link view window",
    description = "Grows an open windowed view (e.g. mailList) by the requested row count and returns the extended snapshot, also broadcast as a viewReplace RuntimeFrame.",
    params(
        ("session_id" = String, Path, description = "Runtime link id"),
        ("view_id" = String, Path, description = "Runtime view id"),
        RuntimeLinkQuery
    ),
    request_body = ExtendRuntimeLinkViewRequest,
    responses(
        (status = 200, description = "The extended view snapshot", body = OpenRuntimeLinkViewResponse),
        (status = 400, description = "View does not support window extension", body = ApiErrorBody),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime link or view", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn extend_runtime_link_view(
    State(state): State<Arc<AppState>>,
    Path((link_id, view_id)): Path<(String, String)>,
    Query(query): Query<RuntimeLinkQuery>,
    Json(request): Json<ExtendRuntimeLinkViewRequest>,
) -> Result<Json<OpenRuntimeLinkViewResponse>, ApiError> {
    let snapshot = state
        .runtime
        .extend_link_view(
            runtime_caller(query.source_id.as_deref()),
            RuntimeLinkId::new(link_id),
            ViewId::new(view_id),
            request.count,
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OpenRuntimeLinkViewResponse {
        view_id: snapshot.view_id.clone(),
        snapshot,
    }))
}
