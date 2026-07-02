use super::*;

/// GET /v1/sources/{source_id}/operations
///
/// @spec docs/L1-outbox#operation-model
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/operations",
    tag = "messages",
    summary = "List pending operations",
    description = "Lists an account's non-terminal outbox operations (pending/failed work), oldest first.",
    params(("source_id" = String, Path, description = "Source (account) identifier")),
    responses(
        (status = 200, description = "Pending operations", body = [Operation]),
        (status = 503, description = "Runtime unavailable", body = ApiErrorBody)
    )
)]
pub async fn list_pending_operations(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
) -> Result<Json<Vec<Operation>>, ApiError> {
    state
        .runtime
        .list_pending_operations(RuntimeCaller::api(), AccountId(source_id))
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// DELETE /v1/sources/{source_id}/operations/{operation_id}
///
/// @spec docs/L1-outbox#operation-model
#[utoipa::path(
    delete,
    path = "/v1/sources/{source_id}/operations/{operation_id}",
    tag = "messages",
    summary = "Discard an outbox operation",
    description = "Removes a queued or failed outbox operation (a user escape hatch for a dead op). In-flight operations cannot be discarded.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("operation_id" = String, Path, description = "Outbox operation identifier")
    ),
    responses(
        (status = 200, description = "Operation discarded", body = OkResponse),
        (status = 400, description = "Operation is in-flight", body = ApiErrorBody),
        (status = 503, description = "Runtime unavailable", body = ApiErrorBody)
    )
)]
pub async fn discard_operation(
    State(state): State<Arc<AppState>>,
    Path((source_id, operation_id)): Path<(String, String)>,
) -> Result<Json<OkResponse>, ApiError> {
    state
        .runtime
        .discard_operation(
            RuntimeCaller::api(),
            AccountId(source_id),
            posthaste_domain_service::OperationId::from(operation_id),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OkResponse { ok: true }))
}

/// POST /v1/sources/{source_id}/operations/{operation_id}/retry
///
/// @spec docs/L1-outbox#operation-model
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/operations/{operation_id}/retry",
    tag = "messages",
    summary = "Retry a failed outbox operation",
    description = "Re-arms a failed outbox operation to pending so the next flush re-attempts it. Only failed operations can be retried.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("operation_id" = String, Path, description = "Outbox operation identifier")
    ),
    responses(
        (status = 200, description = "Operation re-armed", body = OkResponse),
        (status = 400, description = "Operation is not failed", body = ApiErrorBody),
        (status = 503, description = "Runtime unavailable", body = ApiErrorBody)
    )
)]
pub async fn retry_operation(
    State(state): State<Arc<AppState>>,
    Path((source_id, operation_id)): Path<(String, String)>,
) -> Result<Json<OkResponse>, ApiError> {
    state
        .runtime
        .retry_operation(
            RuntimeCaller::api(),
            AccountId(source_id),
            posthaste_domain_service::OperationId::from(operation_id),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OkResponse { ok: true }))
}
