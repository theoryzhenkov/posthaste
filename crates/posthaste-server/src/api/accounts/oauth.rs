use super::oauth_support::*;
use super::*;

/// POST /v1/oauth/start
///
/// Creates a backend-held PKCE authorization session for provider-first setup.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[utoipa::path(
    post,
    path = "/v1/oauth/start",
    tag = "oauth",
    summary = "Start provider OAuth flow",
    description = "Creates a backend-held PKCE authorization session for provider-first setup.",
    request_body = StartProviderOAuthRequest,
    responses(
        (status = 200, description = "Authorization session details", body = StartOAuthResponse),
        (status = 400, description = "Invalid provider or request", body = ApiErrorBody)
    )
)]
pub async fn start_provider_oauth(
    State(state): State<Arc<AppState>>,
    Json(request): Json<StartProviderOAuthRequest>,
) -> Result<Json<StartOAuthResponse>, ApiError> {
    let profile = OAuthProviderProfile::for_provider(&request.provider).ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidProvider,
            "provider does not support built-in OAuth",
        )
    })?;
    let (client_id, client_secret, redirect_uri) = validate_oauth_start_request(
        request.client_id.as_str(),
        request.client_secret.as_deref(),
        request.redirect_uri.as_str(),
    )?;

    let oauth = OAuthTokenService::new().map_err(ServiceError::from)?;
    let session = oauth
        .authorization_session(&profile, client_id, client_secret, redirect_uri)
        .map_err(ServiceError::from)?;
    state
        .oauth_flows
        .insert(
            session.state.clone(),
            PendingOAuthFlow {
                account_id: None,
                profile,
                client_id: client_id.to_string(),
                client_secret: client_secret.map(ToString::to_string),
                redirect_uri: redirect_uri.to_string(),
                pkce_verifier: session.pkce_verifier,
                nonce: session.nonce,
            },
        )
        .await;

    Ok(Json(StartOAuthResponse {
        authorization_url: session.authorization_url,
        state: session.state,
        redirect_uri: session.redirect_uri,
    }))
}

/// POST /v1/accounts/{account_id}/oauth/start
///
/// Creates a backend-held PKCE authorization session for an existing account.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[utoipa::path(
    post,
    path = "/v1/accounts/{account_id}/oauth/start",
    tag = "oauth",
    summary = "Start account OAuth flow",
    description = "Creates a backend-held PKCE authorization session for an existing account.",
    params(("account_id" = String, Path, description = "Account identifier")),
    request_body = StartOAuthRequest,
    responses(
        (status = 200, description = "Authorization session details", body = StartOAuthResponse),
        (status = 400, description = "Invalid account or request", body = ApiErrorBody),
        (status = 404, description = "Account not found", body = ApiErrorBody)
    )
)]
pub async fn start_account_oauth(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
    Json(request): Json<StartOAuthRequest>,
) -> Result<Json<StartOAuthResponse>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let account = state
        .runtime
        .get_account(RuntimeCaller::api(), account_id.clone())
        .await
        .map_err(ApiError::from_runtime_error)?;
    let provider = match &account.connection {
        AccountConnectionOverview::ManualCredentials { provider, .. }
        | AccountConnectionOverview::ManagedOAuth { provider, .. } => provider,
    };
    let profile = OAuthProviderProfile::for_provider(provider).ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidAccount,
            "account provider does not support built-in OAuth",
        )
    })?;
    let (client_id, client_secret, redirect_uri) = validate_oauth_start_request(
        request.client_id.as_str(),
        request.client_secret.as_deref(),
        request.redirect_uri.as_str(),
    )?;

    let oauth = OAuthTokenService::new().map_err(ServiceError::from)?;
    let session = oauth
        .authorization_session(&profile, client_id, client_secret, redirect_uri)
        .map_err(ServiceError::from)?;
    state
        .oauth_flows
        .insert(
            session.state.clone(),
            PendingOAuthFlow {
                account_id: Some(account_id),
                profile,
                client_id: client_id.to_string(),
                client_secret: client_secret.map(ToString::to_string),
                redirect_uri: redirect_uri.to_string(),
                pkce_verifier: session.pkce_verifier,
                nonce: session.nonce,
            },
        )
        .await;

    Ok(Json(StartOAuthResponse {
        authorization_url: session.authorization_url,
        state: session.state,
        redirect_uri: session.redirect_uri,
    }))
}

/// GET /v1/oauth/callback
///
/// Exchanges a provider authorization code for a token set. Provider-first
/// flows create an account from the OIDC identity; existing-account flows
/// store the token set as the account's managed OS secret.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[utoipa::path(
    get,
    path = "/v1/oauth/callback",
    tag = "oauth",
    summary = "Complete OAuth flow",
    description = "Loopback redirect target. Exchanges a provider authorization code for a token \
                   set and returns an HTML page for the browser tab.",
    params(OAuthCallbackQuery),
    responses(
        (status = 200, description = "OAuth completion HTML page", content_type = "text/html"),
        (status = 400, description = "OAuth denied or invalid callback", body = ApiErrorBody)
    )
)]
pub async fn complete_account_oauth(
    State(state): State<Arc<AppState>>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Html<String>, ApiError> {
    if let Some(error) = query.error {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::OauthDenied,
            query.error_description.unwrap_or(error),
        ));
    }
    let code = query
        .code
        .as_deref()
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidOauthCallback,
                "OAuth callback is missing code",
            )
        })?;
    let flow = match state.oauth_flows.begin_completion(&query.state).await {
        OAuthFlowCompletion::Pending(flow) => flow,
        OAuthFlowCompletion::Completing => return Ok(oauth_processing_html()),
        OAuthFlowCompletion::Completed => return Ok(oauth_success_html()),
        OAuthFlowCompletion::Unknown => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidOauthCallback,
                "OAuth callback state is unknown or already used",
            ));
        }
    };
    let oauth = OAuthTokenService::new().map_err(ServiceError::from)?;
    let exchange = oauth
        .exchange_authorization_code(OAuthAuthorizationCodeExchange {
            profile: &flow.profile,
            client_id: &flow.client_id,
            client_secret: flow.client_secret.as_deref(),
            redirect_uri: &flow.redirect_uri,
            code,
            pkce_verifier: &flow.pkce_verifier,
            nonce: &flow.nonce,
            now: time::OffsetDateTime::now_utc(),
        })
        .await
        .map_err(ServiceError::from)?;
    match flow.account_id {
        Some(account_id) => {
            persist_oauth_token_set(&state, &account_id, exchange.token_set).await?;
        }
        None => {
            create_oauth_account_from_exchange(&state, &flow.profile, exchange).await?;
        }
    }

    state.oauth_flows.mark_completed(query.state).await;
    Ok(oauth_success_html())
}
