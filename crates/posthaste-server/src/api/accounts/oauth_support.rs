use super::*;

pub(crate) fn oauth_success_html() -> Html<String> {
    Html(
        "<!doctype html><meta charset=\"utf-8\"><title>Posthaste OAuth</title><p>Authentication complete. You can return to Posthaste.</p>".to_string(),
    )
}

pub(crate) fn oauth_processing_html() -> Html<String> {
    Html(
        "<!doctype html><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"1\"><title>Posthaste OAuth</title><p>Authentication is still completing. This page will refresh automatically.</p>".to_string(),
    )
}

pub(crate) fn validate_oauth_start_request<'a>(
    client_id: &'a str,
    client_secret: Option<&'a str>,
    redirect_uri: &'a str,
) -> Result<(&'a str, Option<&'a str>, &'a str), ApiError> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidOauthRequest,
            "clientId is required",
        ));
    }
    let redirect_uri = redirect_uri.trim();
    if redirect_uri.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidOauthRequest,
            "redirectUri is required",
        ));
    }
    Ok((
        client_id,
        client_secret
            .map(str::trim)
            .filter(|client_secret| !client_secret.is_empty()),
        redirect_uri,
    ))
}

pub(crate) async fn create_oauth_account_from_exchange(
    state: &Arc<AppState>,
    profile: &OAuthProviderProfile,
    exchange: OAuthExchangeResult,
) -> Result<AccountId, ApiError> {
    state
        .runtime
        .create_oauth_account_from_exchange(profile, exchange)
        .await
        .map(|account| account.id)
        .map_err(ApiError::from_runtime_error)
}

pub(crate) async fn persist_oauth_token_set(
    state: &Arc<AppState>,
    account_id: &AccountId,
    token_set: OAuthTokenSet,
) -> Result<(), ApiError> {
    state
        .runtime
        .persist_oauth_token_set(account_id.clone(), token_set)
        .await
        .map(|_| ())
        .map_err(ApiError::from_runtime_error)
}
