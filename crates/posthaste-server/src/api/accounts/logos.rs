use super::*;

/// POST /v1/accounts/{account_id}/logo
///
/// Stores a user-uploaded account logo under the config root and updates the
/// account appearance to reference it.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[utoipa::path(
    post,
    path = "/v1/accounts/{account_id}/logo",
    tag = "accounts",
    summary = "Upload account logo",
    description = "Stores a user-uploaded account logo (PNG, JPEG, WebP, or GIF) and updates the \
                   account appearance to reference it. The request body is the raw image bytes.",
    params(("account_id" = String, Path, description = "Account identifier")),
    request_body(content = [u8], description = "Raw image bytes", content_type = "application/octet-stream"),
    responses(
        (status = 200, description = "The updated account", body = AccountOverview),
        (status = 400, description = "Invalid or oversized image", body = ApiErrorBody),
        (status = 404, description = "Account not found", body = ApiErrorBody)
    )
)]
pub async fn upload_account_logo(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Json<AccountOverview>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let mut account = load_account(state.as_ref(), &account_id)?;

    if bytes.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidAccountLogo,
            "account logo file is empty",
        ));
    }
    if bytes.len() > MAX_ACCOUNT_LOGO_BYTES {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidAccountLogo,
            "account logo file is too large",
        ));
    }

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .unwrap_or("");
    let extension = account_logo_extension(content_type)?;
    fs::create_dir_all(&state.account_logo_root)
        .await
        .map_err(|err| internal_error(format!("failed to create account logo directory: {err}")))?;
    let image_id = uuid::Uuid::new_v4().simple().to_string();
    let path = state
        .account_logo_root
        .join(format!("{image_id}.{extension}"));
    fs::write(&path, &bytes)
        .await
        .map_err(|err| internal_error(format!("failed to write account logo: {err}")))?;

    let previous_image_id = match &account.appearance {
        Some(AccountAppearance::Image { image_id, .. }) => Some(image_id.clone()),
        _ => None,
    };
    let (initials, color_hue) = account_appearance_fallback_parts(&account);
    account.appearance = Some(AccountAppearance::Image {
        image_id: image_id.clone(),
        initials,
        color_hue,
    });
    account.updated_at = domain_now_iso8601().map_err(internal_error)?;
    validate_account_settings(&account)?;
    if let Err(error) = state.service.save_source(&account) {
        let _ = delete_account_logo_file(state.as_ref(), &image_id).await;
        return Err(ApiError::from_service_error(error));
    }
    append_and_publish_account_event(&state, &account_id, EVENT_TOPIC_ACCOUNT_UPDATED)
        .map_err(store_error_to_api)?;
    if let Some(previous_image_id) = previous_image_id {
        if previous_image_id != image_id {
            let _ = delete_account_logo_file(state.as_ref(), &previous_image_id).await;
        }
    }

    let settings = state
        .service
        .get_app_settings()
        .map_err(ApiError::from_service_error)?;
    Ok(Json(account_overview(&state, &settings, account).await))
}

/// GET /v1/account-assets/logos/{image_id}
///
/// @spec docs/L1-api#account-crud-lifecycle
#[utoipa::path(
    get,
    path = "/v1/account-assets/logos/{image_id}",
    tag = "accounts",
    summary = "Get account logo",
    description = "Returns the stored account logo image bytes.",
    params(("image_id" = String, Path, description = "Logo image identifier")),
    responses(
        (status = 200, description = "Logo image bytes", content_type = "image/*", body = [u8]),
        (status = 404, description = "Logo not found", body = ApiErrorBody)
    )
)]
pub async fn get_account_logo(
    State(state): State<Arc<AppState>>,
    Path(image_id): Path<String>,
) -> Result<Response, ApiError> {
    validate_logo_image_id(&image_id)?;
    for (extension, content_type) in ACCOUNT_LOGO_MIME_TYPES {
        let path = state
            .account_logo_root
            .join(format!("{image_id}.{extension}"));
        if path.exists() {
            let bytes = fs::read(path)
                .await
                .map_err(|err| internal_error(format!("failed to read account logo: {err}")))?;
            let mut response = Response::new(Body::from(bytes));
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=86400"),
            );
            return Ok(response);
        }
    }
    Err(ApiError::new(
        StatusCode::NOT_FOUND,
        ApiErrorCode::NotFound,
        "account logo not found",
    ))
}

/// DELETE /v1/accounts/{account_id}
///
/// Removes the managed OS keyring secret, stops the supervisor runtime,
/// deletes the config file, and emits an `account.deleted` event.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[utoipa::path(
    delete,
    path = "/v1/accounts/{account_id}",
    tag = "accounts",
    summary = "Delete account",
    description = "Removes the managed keyring secret, stops the runtime, deletes config, and \
                   emits an account.deleted event.",
    params(("account_id" = String, Path, description = "Account identifier")),
    responses(
        (status = 200, description = "Account deleted", body = OkResponse),
        (status = 404, description = "Account not found", body = ApiErrorBody)
    )
)]
pub async fn delete_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let account = load_account(state.as_ref(), &account_id)?;
    let logo_image_id = match &account.appearance {
        Some(AccountAppearance::Image { image_id, .. }) => Some(image_id.clone()),
        _ => None,
    };
    delete_managed_secret(state.as_ref(), account.transport.secret_ref.as_ref())?;
    state.supervisor.remove_account(&account_id).await;
    state
        .service
        .delete_source(&account_id)
        .map_err(ApiError::from_service_error)?;
    append_and_publish_account_event(&state, &account_id, EVENT_TOPIC_ACCOUNT_DELETED)
        .map_err(store_error_to_api)?;
    if let Some(image_id) = logo_image_id {
        let _ = delete_account_logo_file(state.as_ref(), &image_id).await;
    }
    Ok(Json(OkResponse { ok: true }))
}

const ACCOUNT_LOGO_MIME_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("webp", "image/webp"),
    ("gif", "image/gif"),
];

pub(super) fn account_logo_extension(content_type: &str) -> Result<&'static str, ApiError> {
    match content_type {
        "image/png" => Ok("png"),
        "image/jpeg" => Ok("jpg"),
        "image/webp" => Ok("webp"),
        "image/gif" => Ok("gif"),
        _ => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidAccountLogo,
            "account logo must be a PNG, JPEG, WebP, or GIF image",
        )),
    }
}

pub(super) fn account_appearance_fallback_parts(account: &AccountSettings) -> (String, u16) {
    let appearance = account
        .appearance
        .clone()
        .unwrap_or_else(|| default_account_appearance(account));
    match normalize_account_appearance(appearance) {
        AccountAppearance::Initials {
            initials,
            color_hue,
        } => (initials, color_hue),
        AccountAppearance::Image {
            initials,
            color_hue,
            ..
        } => (initials, color_hue),
    }
}

pub(super) fn account_appearance_image_id(account: &AccountSettings) -> Option<String> {
    match &account.appearance {
        Some(AccountAppearance::Image { image_id, .. }) => Some(image_id.clone()),
        _ => None,
    }
}

pub(super) async fn delete_account_logo_file(
    state: &AppState,
    image_id: &str,
) -> Result<(), ApiError> {
    validate_logo_image_id(image_id)?;
    for (extension, _) in ACCOUNT_LOGO_MIME_TYPES {
        let path = state
            .account_logo_root
            .join(format!("{image_id}.{extension}"));
        match fs::remove_file(path).await {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(internal_error(format!(
                    "failed to delete previous account logo: {error}"
                )));
            }
        }
    }
    Ok(())
}

// ---- Capability-token minting (`POST /v1/auth/tokens`) ----
