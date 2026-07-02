use super::*;

#[cfg(test)]
use posthaste_domain_service::{
    AccountDriver, AccountSettings, AccountTransportSettings, ImapTransportSettings,
    ProviderAuthKind, ProviderHint, SecretRef, SmtpTransportSettings,
};

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
    state: &Arc<OAuthState>,
    profile: &OAuthProviderProfile,
    exchange: OAuthExchangeResult,
) -> Result<AccountId, ApiError> {
    state
        .oauth_mutations
        .as_ref()
        .ok_or_else(|| {
            ApiError::from_runtime_error(RuntimeError::runtime_not_ready(
                "account mutation runtime is not available",
            ))
        })?
        .create_oauth_account_from_exchange(profile, exchange)
        .await
        .map(|account| account.id)
        .map_err(ApiError::from_runtime_error)
}

#[cfg(test)]
pub(crate) fn oauth_account_settings(
    account_id: AccountId,
    provider: ProviderHint,
    name: String,
    identity_email: String,
    email_patterns: Vec<String>,
    secret_ref: SecretRef,
    timestamp: String,
) -> Result<AccountSettings, ApiError> {
    let (imap, smtp) = oauth_provider_mail_transport(&provider)?;
    Ok(AccountSettings {
        id: account_id,
        name,
        full_name: None,
        signature: None,
        email_patterns,
        driver: AccountDriver::ImapSmtp,
        enabled: true,
        appearance: None,
        transport: AccountTransportSettings {
            provider,
            auth: ProviderAuthKind::OAuth2,
            base_url: None,
            username: Some(identity_email),
            secret_ref: Some(secret_ref),
            imap: Some(imap),
            smtp: Some(smtp),
        },
        created_at: timestamp.clone(),
        updated_at: timestamp,
    })
}

#[cfg(test)]
pub(crate) fn oauth_provider_mail_transport(
    provider: &ProviderHint,
) -> Result<(ImapTransportSettings, SmtpTransportSettings), ApiError> {
    OAuthProviderProfile::for_provider(provider)
        .and_then(|profile| profile.default_mail_transport())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidProvider,
                "provider does not support built-in OAuth account creation",
            )
        })
}

pub(crate) async fn persist_oauth_token_set(
    state: &Arc<OAuthState>,
    account_id: &AccountId,
    token_set: OAuthTokenSet,
) -> Result<(), ApiError> {
    state
        .oauth_mutations
        .as_ref()
        .ok_or_else(|| {
            ApiError::from_runtime_error(RuntimeError::runtime_not_ready(
                "account mutation runtime is not available",
            ))
        })?
        .persist_oauth_token_set(account_id.clone(), token_set)
        .await
        .map(|_| ())
        .map_err(ApiError::from_runtime_error)
}
