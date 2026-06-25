use super::*;

/// Request body for a manual source sync command.
///
/// @spec docs/L1-api#sync-and-events
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TriggerSyncRequest {
    #[serde(default)]
    pub mode: SyncMode,
}

/// Response from `POST /v1/sources/{id}/commands/sync`.
///
/// @spec docs/L1-api#sync-and-events
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TriggerSyncResponse {
    pub ok: bool,
    pub event_count: usize,
    pub mode: String,
}

/// POST /v1/sources/{source_id}/commands/sync
///
/// @spec docs/L1-api#sync-and-events
/// @spec docs/L1-sync#sync-loop
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/commands/sync",
    tag = "sync",
    summary = "Trigger sync",
    description = "Runs a manual sync for a source and reports the number of events emitted.",
    params(("source_id" = String, Path, description = "Source (account) identifier")),
    // NOTE: the handler accepts an absent body (defaults to incremental sync). utoipa's
    // path macro can't emit `requestBody.required: false`, so optionality is documented here.
    request_body(content = TriggerSyncRequest,
        description = "Optional. Defaults to an incremental sync when the body is omitted."),
    responses(
        (status = 200, description = "Sync result", body = TriggerSyncResponse),
        (status = 404, description = "Source not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn trigger_sync(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    request: Option<Json<TriggerSyncRequest>>,
) -> Result<Json<TriggerSyncResponse>, ApiError> {
    let account_id = AccountId(source_id);
    let mode = request
        .map(|Json(request)| request.mode)
        .unwrap_or_default();
    let event_count = state
        .runtime
        .sync_account(RuntimeCaller::api(), account_id, mode)
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(TriggerSyncResponse {
        ok: true,
        event_count,
        mode: mode.as_str().to_string(),
    }))
}
