use super::*;

pub(crate) fn oauth_client(
    profile: &OAuthProviderProfile,
    client_id: &str,
    client_secret: Option<&str>,
    redirect_uri: &str,
) -> Result<OAuthClient, GatewayError> {
    let mut client = oauth2::Client::new(ClientId::new(client_id.to_string()))
        .set_auth_uri(AuthUrl::new(profile.auth_url.to_string()).map_err(invalid_oauth_url)?)
        .set_token_uri(TokenUrl::new(profile.token_url.to_string()).map_err(invalid_oauth_url)?)
        .set_redirect_uri(RedirectUrl::new(redirect_uri.to_string()).map_err(invalid_oauth_url)?);
    if let Some(client_secret) = client_secret.filter(|secret| !secret.trim().is_empty()) {
        client = client
            .set_client_secret(ClientSecret::new(client_secret.trim().to_string()))
            .set_auth_type(AuthType::RequestBody);
    }
    Ok(client)
}

pub(crate) fn expires_at_from_duration(
    now: OffsetDateTime,
    expires_in: Option<std::time::Duration>,
) -> Result<Option<String>, GatewayError> {
    expires_in
        .map(|duration| {
            let duration = Duration::try_from(duration).map_err(|error| {
                GatewayError::Rejected(format!("invalid OAuth token duration: {error}"))
            })?;
            (now + duration).format(&Rfc3339).map_err(|error| {
                GatewayError::Rejected(format!("invalid OAuth token expiry: {error}"))
            })
        })
        .transpose()
}

pub(crate) fn invalid_oauth_url(error: oauth2::url::ParseError) -> GatewayError {
    GatewayError::Rejected(format!("invalid OAuth provider URL: {error}"))
}

pub(crate) fn oauth_request_error<E>(error: E) -> GatewayError
where
    E: std::fmt::Display,
{
    let message = error.to_string();
    if message.contains("invalid_grant") || message.contains("unauthorized_client") {
        GatewayError::Auth
    } else {
        GatewayError::Network(message)
    }
}

/// Terminality of an OAuth refresh failure (D102 / A2, reusing the M29
/// [`Terminality`] taxonomy).
///
/// The `invalid_grant` / `unauthorized_client` class that [`oauth_request_error`]
/// types as [`GatewayError::Auth`] is **Permanent**: the grant is revoked or the
/// refresh token has been consumed, so no retry, reconnect, or later tick can
/// recover it — only re-authentication can. Every other refresh failure (a
/// network blip, a 5xx at the IdP) is **Transient** and left for the next tick to
/// retry. The refresh tick uses the Permanent verdict to flip the account to
/// `AuthError` immediately, instead of swallowing the error as a warning and
/// waiting for a later connection rebuild to surface it.
pub(crate) fn oauth_refresh_terminality(error: &GatewayError) -> Terminality {
    match error {
        GatewayError::Auth => Terminality::Permanent,
        _ => Terminality::Transient,
    }
}

pub(crate) fn invalid_openid_token<E>(error: E) -> GatewayError
where
    E: std::fmt::Display,
{
    GatewayError::Rejected(format!("OAuth identity token is invalid: {error}"))
}
