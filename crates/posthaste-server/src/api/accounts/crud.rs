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
    let CreateAccountRequest {
        id,
        name,
        full_name,
        email_patterns,
        driver,
        enabled,
        appearance,
        transport,
        secret,
    } = request;
    let email_patterns = normalize_email_patterns(&email_patterns);
    let account_id = match id {
        Some(id) if !id.trim().is_empty() => AccountId::from(id.trim()),
        _ => {
            let seed = generate_account_id_seed(&name, &email_patterns);
            allocate_unique_account_id(state.as_ref(), &seed)?
        }
    };
    if state
        .service
        .get_source(&account_id)
        .map_err(ApiError::from_service_error)?
        .is_some()
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            ApiErrorCode::Conflict,
            "account already exists",
        ));
    }

    let timestamp = domain_now_iso8601().map_err(internal_error)?;
    let mut account = AccountSettings {
        id: account_id.clone(),
        name: name.trim().to_string(),
        full_name: normalize_optional(full_name),
        email_patterns,
        driver: driver.unwrap_or(AccountDriver::Jmap),
        enabled: enabled.unwrap_or(true),
        appearance: appearance.map(normalize_account_appearance),
        transport: transport.into(),
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };
    account.transport.secret_ref =
        decide_secret_instruction(&account.id, None, &secret)?.resolved_secret_ref(None);
    validate_account_settings(&account)?;
    apply_secret_instruction(state.as_ref(), &mut account, None, &secret)?;
    persist_new_account(&state, &account, EVENT_TOPIC_ACCOUNT_CREATED).await?;

    let settings = state
        .service
        .get_app_settings()
        .map_err(ApiError::from_service_error)?;
    Ok(Json(account_overview(&state, &settings, account).await))
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
    let mut account = load_account(state.as_ref(), &account_id)?;
    let previous_image_id = account_appearance_image_id(&account);
    apply_account_patch(&mut account, &request);
    account.updated_at = domain_now_iso8601().map_err(internal_error)?;
    let existing_secret_ref = account.transport.secret_ref.clone();
    let secret_request = request.secret.unwrap_or_default();
    account.transport.secret_ref =
        decide_secret_instruction(&account.id, existing_secret_ref.as_ref(), &secret_request)?
            .resolved_secret_ref(existing_secret_ref.as_ref());
    validate_account_settings(&account)?;
    let defer_secret_clear = secret_request.mode == SecretWriteMode::Clear;
    if !defer_secret_clear {
        apply_secret_instruction(
            state.as_ref(),
            &mut account,
            existing_secret_ref.as_ref(),
            &secret_request,
        )?;
    }

    state
        .service
        .save_source(&account)
        .map_err(ApiError::from_service_error)?;
    if defer_secret_clear {
        apply_secret_instruction(
            state.as_ref(),
            &mut account,
            existing_secret_ref.as_ref(),
            &secret_request,
        )?;
    }
    state.supervisor.start_account(&account).await;
    append_and_publish_account_event(&state, &account_id, EVENT_TOPIC_ACCOUNT_UPDATED)
        .map_err(store_error_to_api)?;
    let next_image_id = account_appearance_image_id(&account);
    if previous_image_id != next_image_id {
        if let Some(previous_image_id) = previous_image_id {
            let _ = delete_account_logo_file(state.as_ref(), &previous_image_id).await;
        }
    }

    let settings = state
        .service
        .get_app_settings()
        .map_err(ApiError::from_service_error)?;
    Ok(Json(account_overview(&state, &settings, account).await))
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
    let account_id = AccountId::from(account_id.as_str());
    let account = load_account(state.as_ref(), &account_id)?;
    let result = state
        .supervisor
        .verify_account(&account)
        .await
        .map_err(ApiError::from_service_error)?;
    Ok(Json(VerificationResponse {
        ok: result.ok,
        identity_email: result.identity.map(|identity| identity.email),
        push_supported: result.push_supported,
    }))
}
