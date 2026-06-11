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
    let diff = state
        .service
        .reload_config()
        .map_err(ApiError::from_service_error)?;

    // Apply diff to supervisor
    for id in &diff.removed_sources {
        state.supervisor.remove_account(id).await;
    }
    for id in diff.added_sources.iter().chain(diff.changed_sources.iter()) {
        let source = state
            .service
            .get_source(id)
            .map_err(ApiError::from_service_error)?;
        if let Some(source) = source {
            state.supervisor.start_account(&source).await;
        }
    }

    let mut resources = vec![ResourceChange::config_reloaded()];
    resources.extend(
        diff.added_sources
            .iter()
            .map(|id| ResourceChange::account(ResourceOperation::Created, id)),
    );
    resources.extend(
        diff.changed_sources
            .iter()
            .map(|id| ResourceChange::account(ResourceOperation::Updated, id)),
    );
    resources.extend(
        diff.removed_sources
            .iter()
            .map(|id| ResourceChange::account(ResourceOperation::Deleted, id)),
    );
    append_and_publish_config_event(
        &state,
        EVENT_TOPIC_CONFIG_RELOADED,
        resources,
        json!({
            "addedSourceCount": diff.added_sources.len(),
            "changedSourceCount": diff.changed_sources.len(),
            "removedSourceCount": diff.removed_sources.len(),
        }),
    )
    .map_err(store_error_to_api)?;

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
    let account_id = AccountId::from(account_id.as_str());
    let mut account = load_account(state.as_ref(), &account_id)?;
    account.enabled = enabled;
    account.updated_at = domain_now_iso8601().map_err(internal_error)?;
    state
        .service
        .save_source(&account)
        .map_err(ApiError::from_service_error)?;
    state.supervisor.start_account(&account).await;
    append_and_publish_account_event(&state, &account_id, EVENT_TOPIC_ACCOUNT_UPDATED)
        .map_err(store_error_to_api)?;
    Ok(Json(OkResponse { ok: true }))
}
