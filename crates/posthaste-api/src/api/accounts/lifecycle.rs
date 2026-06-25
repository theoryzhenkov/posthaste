use super::*;

/// POST /v1/accounts/{account_id}/enable
///
/// @spec docs/L1-api#account-crud-lifecycle
#[utoipa::path(
    post,
    path = "/v1/accounts/{account_id}/enable",
    tag = "accounts",
    summary = "Enable account",
    description = "Sets the account enabled flag and restarts the runtime.",
    params(("account_id" = String, Path, description = "Account identifier")),
    responses(
        (status = 200, description = "Account enabled", body = OkResponse),
        (status = 404, description = "Account not found", body = ApiErrorBody)
    )
)]
pub async fn enable_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    set_account_enabled(state, account_id, true).await
}

/// POST /v1/accounts/{account_id}/disable
///
/// @spec docs/L1-api#account-crud-lifecycle
#[utoipa::path(
    post,
    path = "/v1/accounts/{account_id}/disable",
    tag = "accounts",
    summary = "Disable account",
    description = "Clears the account enabled flag and restarts the runtime.",
    params(("account_id" = String, Path, description = "Account identifier")),
    responses(
        (status = 200, description = "Account disabled", body = OkResponse),
        (status = 404, description = "Account not found", body = ApiErrorBody)
    )
)]
pub async fn disable_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    set_account_enabled(state, account_id, false).await
}

/// POST /v1/config:reload
///
/// Re-reads config from disk, diffs against the in-memory snapshot, and
/// starts/stops supervisor runtimes for changed accounts.
///
/// @spec docs/L1-api#sync-and-events
/// @spec docs/L1-accounts#configdiff
#[utoipa::path(
    post,
    path = "/v1/config:reload",
    tag = "sync",
    summary = "Reload configuration",
    description = "Re-reads config from disk, diffs against the in-memory snapshot, and \
                   starts/stops runtimes for changed accounts.",
    responses(
        (status = 200, description = "Configuration reloaded", body = OkResponse),
        (status = 400, description = "Configuration invalid", body = ApiErrorBody),
        (status = 500, description = "Configuration reload failed", body = ApiErrorBody)
    )
)]
pub async fn reload_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<OkResponse>, ApiError> {
    state
        .runtime
        .reload_config(RuntimeCaller::api())
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OkResponse { ok: true }))
}

/// Toggle the `enabled` flag on an account, re-persist, and restart the supervisor.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub(super) async fn set_account_enabled(
    state: Arc<AppState>,
    account_id: String,
    enabled: bool,
) -> Result<Json<OkResponse>, ApiError> {
    state
        .runtime
        .set_account_enabled(
            RuntimeCaller::api(),
            AccountId::from(account_id.as_str()),
            enabled,
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OkResponse { ok: true }))
}
