use super::*;

/// GET /v1/accounts
///
/// @spec docs/L1-api#accounts
#[utoipa::path(
    get,
    path = "/v1/accounts",
    tag = "accounts",
    summary = "List accounts",
    description = "Returns all configured accounts with their runtime overview.",
    responses(
        (status = 200, description = "All configured accounts", body = [AccountOverview]),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn list_accounts(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<AccountOverview>>, ApiError> {
    state
        .runtime
        .list_accounts(RuntimeCaller::api())
        .await
        .map(|accounts| Json(accounts.items))
        .map_err(ApiError::from_runtime_error)
}

/// GET /v1/accounts/{account_id}
///
/// @spec docs/L1-api#accounts
#[utoipa::path(
    get,
    path = "/v1/accounts/{account_id}",
    tag = "accounts",
    summary = "Get account",
    description = "Returns a single account with its runtime overview.",
    params(("account_id" = String, Path, description = "Account identifier")),
    responses(
        (status = 200, description = "The requested account", body = AccountOverview),
        (status = 404, description = "Account not found", body = ApiErrorBody)
    )
)]
pub async fn get_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<AccountOverview>, ApiError> {
    state
        .runtime
        .get_account(RuntimeCaller::api(), AccountId::from(account_id.as_str()))
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// POST /v1/accounts
///
/// Validates uniqueness, applies secret instruction, persists config, starts
/// the supervisor runtime, and emits an `account.created` event.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[utoipa::path(
    post,
    path = "/v1/accounts",
    tag = "accounts",
    summary = "Create account",
    description = "Validates uniqueness, applies the secret instruction, persists config, \
                   starts the runtime, and emits an account.created event.",
    request_body = CreateAccountRequest,
    responses(
        (status = 200, description = "The created account", body = AccountOverview),
        (status = 400, description = "Validation failed", body = ApiErrorBody),
        (status = 409, description = "Account already exists", body = ApiErrorBody)
    )
)]
pub async fn create_account(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateAccountRequest>,
) -> Result<Json<AccountOverview>, ApiError> {
    state
        .runtime
        .create_account(RuntimeCaller::api(), request.into())
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// PATCH /v1/accounts/{account_id}
///
/// Sparse-merges provided fields into the existing account and restarts
/// the supervisor runtime.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[utoipa::path(
    patch,
    path = "/v1/accounts/{account_id}",
    tag = "accounts",
    summary = "Update account",
    description = "Sparse-merges provided fields into the existing account and restarts the runtime.",
    params(("account_id" = String, Path, description = "Account identifier")),
    request_body = PatchAccountRequest,
    responses(
        (status = 200, description = "The updated account", body = AccountOverview),
        (status = 400, description = "Validation failed", body = ApiErrorBody),
        (status = 404, description = "Account not found", body = ApiErrorBody)
    )
)]
pub async fn patch_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
    Json(request): Json<PatchAccountRequest>,
) -> Result<Json<AccountOverview>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let previous_image_id = load_account(state.as_ref(), &account_id)
        .ok()
        .and_then(|account| account_appearance_image_id(&account));
    let account = state
        .runtime
        .patch_account(RuntimeCaller::api(), account_id, request.into())
        .await
        .map_err(ApiError::from_runtime_error)?;
    let next_image_id = account_appearance_image_id_from_overview(&account);
    if previous_image_id != next_image_id {
        if let Some(previous_image_id) = previous_image_id {
            let _ = delete_account_logo_file(state.as_ref(), &previous_image_id).await;
        }
    }
    Ok(Json(account))
}

/// POST /v1/accounts/{account_id}/verify
///
/// Attempts JMAP session discovery and reports identity and push support.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[utoipa::path(
    post,
    path = "/v1/accounts/{account_id}/verify",
    tag = "accounts",
    summary = "Verify account",
    description = "Attempts JMAP session discovery and reports identity and push support.",
    params(("account_id" = String, Path, description = "Account identifier")),
    responses(
        (status = 200, description = "Verification result", body = VerificationResponse),
        (status = 404, description = "Account not found", body = ApiErrorBody),
        (status = 502, description = "Gateway verification failed", body = ApiErrorBody)
    )
)]
pub async fn verify_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<VerificationResponse>, ApiError> {
    let result = state
        .runtime
        .verify_account(RuntimeCaller::api(), AccountId::from(account_id.as_str()))
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(VerificationResponse {
        ok: result.ok,
        identity_email: result.identity_email,
        push_supported: result.push_supported,
    }))
}

fn account_appearance_image_id_from_overview(account: &AccountOverview) -> Option<String> {
    match &account.appearance {
        AccountAppearance::Image { image_id, .. } => Some(image_id.clone()),
        AccountAppearance::Initials { .. } => None,
    }
}
