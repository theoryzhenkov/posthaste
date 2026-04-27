use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use posthaste_domain::{GatewayError, ProviderHint};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

const OAUTH_REFRESH_SKEW_SECONDS: i64 = 300;

/// OAuth 2.0 provider endpoints and default mail scopes.
///
/// The flow follows the OAuth 2.1 security posture before OAuth 2.1 is final:
/// authorization code only, PKCE required, no password or implicit grant.
///
/// @spec docs/L0-providers#authentication
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthProviderProfile {
    pub provider: ProviderHint,
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub scopes: &'static [&'static str],
    pub extra_authorization_params: &'static [(&'static str, &'static str)],
}

impl OAuthProviderProfile {
    pub fn for_provider(provider: &ProviderHint) -> Option<Self> {
        match provider {
            ProviderHint::Gmail => Some(Self {
                provider: ProviderHint::Gmail,
                auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
                token_url: "https://oauth2.googleapis.com/token",
                scopes: &["https://mail.google.com/"],
                extra_authorization_params: &[("access_type", "offline"), ("prompt", "consent")],
            }),
            ProviderHint::Outlook => Some(Self {
                provider: ProviderHint::Outlook,
                auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
                token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
                scopes: &[
                    "offline_access",
                    "https://outlook.office.com/IMAP.AccessAsUser.All",
                    "https://outlook.office.com/SMTP.Send",
                ],
                extra_authorization_params: &[],
            }),
            ProviderHint::Generic | ProviderHint::Icloud => None,
        }
    }
}

/// Serializable OAuth token bundle stored as the account secret value.
///
/// The API never returns this payload. It is resolved only inside the backend
/// and converted to a short-lived access token before opening XOAUTH2 sessions.
///
/// @spec docs/L1-api#secret-management
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthTokenSet {
    #[serde(default = "oauth_secret_type")]
    pub r#type: String,
    pub provider: ProviderHint,
    pub client_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl OAuthTokenSet {
    pub fn decode(secret: &str) -> Result<Self, GatewayError> {
        let token_set: Self = serde_json::from_str(secret).map_err(|error| {
            GatewayError::Rejected(format!("invalid OAuth token secret: {error}"))
        })?;
        if token_set.r#type != "oauth2" {
            return Err(GatewayError::Rejected(format!(
                "invalid OAuth token secret type: {}",
                token_set.r#type
            )));
        }
        Ok(token_set)
    }

    pub fn encode(&self) -> Result<String, GatewayError> {
        serde_json::to_string(self)
            .map_err(|error| GatewayError::Rejected(format!("invalid OAuth token secret: {error}")))
    }

    pub fn expires_at(&self) -> Result<Option<OffsetDateTime>, GatewayError> {
        self.expires_at
            .as_deref()
            .map(|expires_at| {
                OffsetDateTime::parse(expires_at, &Rfc3339).map_err(|error| {
                    GatewayError::Rejected(format!("invalid OAuth token expiry: {error}"))
                })
            })
            .transpose()
    }

    pub fn requires_refresh_at(&self, now: OffsetDateTime) -> Result<bool, GatewayError> {
        let Some(expires_at) = self.expires_at()? else {
            return Ok(false);
        };
        Ok(expires_at <= now + Duration::seconds(OAUTH_REFRESH_SKEW_SECONDS))
    }
}

fn oauth_secret_type() -> String {
    "oauth2".to_string()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthAuthorizationSession {
    pub authorization_url: String,
    pub state: String,
    pub pkce_verifier: String,
    pub redirect_uri: String,
}

#[derive(Clone)]
pub struct OAuthTokenService {
    http_client: oauth2::reqwest::Client,
}

impl OAuthTokenService {
    pub fn new() -> Result<Self, GatewayError> {
        let http_client = oauth2::reqwest::ClientBuilder::new()
            .redirect(oauth2::reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| GatewayError::Rejected(format!("OAuth HTTP client: {error}")))?;
        Ok(Self { http_client })
    }

    pub fn authorization_session(
        &self,
        profile: &OAuthProviderProfile,
        client_id: &str,
        redirect_uri: &str,
    ) -> Result<OAuthAuthorizationSession, GatewayError> {
        let client = oauth_client(profile, client_id, redirect_uri)?;
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let mut request = client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge);

        for scope in profile.scopes {
            request = request.add_scope(Scope::new((*scope).to_string()));
        }
        for (name, value) in profile.extra_authorization_params {
            request = request.add_extra_param(*name, *value);
        }

        let (authorization_url, state) = request.url();
        Ok(OAuthAuthorizationSession {
            authorization_url: authorization_url.to_string(),
            state: state.secret().to_string(),
            pkce_verifier: pkce_verifier.secret().to_string(),
            redirect_uri: redirect_uri.to_string(),
        })
    }

    pub async fn exchange_authorization_code(
        &self,
        profile: &OAuthProviderProfile,
        client_id: &str,
        redirect_uri: &str,
        code: &str,
        pkce_verifier: &str,
        now: OffsetDateTime,
    ) -> Result<OAuthTokenSet, GatewayError> {
        let client = oauth_client(profile, client_id, redirect_uri)?;
        let token_response = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier.to_string()))
            .request_async(&self.http_client)
            .await
            .map_err(oauth_request_error)?;

        Ok(OAuthTokenSet {
            r#type: oauth_secret_type(),
            provider: profile.provider.clone(),
            client_id: client_id.to_string(),
            access_token: token_response.access_token().secret().to_string(),
            refresh_token: token_response
                .refresh_token()
                .map(|token| token.secret().to_string()),
            expires_at: expires_at_from_duration(now, token_response.expires_in())?,
            scopes: token_response
                .scopes()
                .map(|scopes| scopes.iter().map(|scope| scope.to_string()).collect())
                .unwrap_or_else(|| {
                    profile
                        .scopes
                        .iter()
                        .map(|scope| (*scope).to_string())
                        .collect()
                }),
        })
    }

    pub async fn access_token(
        &self,
        token_set: &OAuthTokenSet,
        now: OffsetDateTime,
    ) -> Result<OAuthAccessToken, GatewayError> {
        if !token_set.requires_refresh_at(now)? {
            return Ok(OAuthAccessToken {
                token: token_set.access_token.clone(),
                updated_token_set: None,
            });
        }
        let refresh_token = token_set.refresh_token.as_ref().ok_or(GatewayError::Auth)?;
        let profile = OAuthProviderProfile::for_provider(&token_set.provider).ok_or_else(|| {
            GatewayError::Rejected(format!(
                "OAuth refresh is not configured for provider {:?}",
                token_set.provider
            ))
        })?;
        let client = oauth_client(&profile, &token_set.client_id, "http://127.0.0.1/unused")?;
        let token_response = client
            .exchange_refresh_token(&RefreshToken::new(refresh_token.clone()))
            .request_async(&self.http_client)
            .await
            .map_err(oauth_request_error)?;

        let updated = OAuthTokenSet {
            r#type: oauth_secret_type(),
            provider: token_set.provider.clone(),
            client_id: token_set.client_id.clone(),
            access_token: token_response.access_token().secret().to_string(),
            refresh_token: token_response
                .refresh_token()
                .map(|token| token.secret().to_string())
                .or_else(|| token_set.refresh_token.clone()),
            expires_at: expires_at_from_duration(now, token_response.expires_in())?,
            scopes: token_response
                .scopes()
                .map(|scopes| scopes.iter().map(|scope| scope.to_string()).collect())
                .unwrap_or_else(|| token_set.scopes.clone()),
        };

        Ok(OAuthAccessToken {
            token: updated.access_token.clone(),
            updated_token_set: Some(updated),
        })
    }
}

pub struct OAuthAccessToken {
    pub token: String,
    pub updated_token_set: Option<OAuthTokenSet>,
}

fn oauth_client(
    profile: &OAuthProviderProfile,
    client_id: &str,
    redirect_uri: &str,
) -> Result<
    oauth2::Client<
        oauth2::StandardErrorResponse<oauth2::basic::BasicErrorResponseType>,
        oauth2::StandardTokenResponse<oauth2::EmptyExtraTokenFields, oauth2::basic::BasicTokenType>,
        oauth2::StandardTokenIntrospectionResponse<
            oauth2::EmptyExtraTokenFields,
            oauth2::basic::BasicTokenType,
        >,
        oauth2::StandardRevocableToken,
        oauth2::StandardErrorResponse<oauth2::RevocationErrorResponseType>,
        oauth2::EndpointSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointSet,
    >,
    GatewayError,
> {
    Ok(BasicClient::new(ClientId::new(client_id.to_string()))
        .set_auth_uri(AuthUrl::new(profile.auth_url.to_string()).map_err(invalid_oauth_url)?)
        .set_token_uri(TokenUrl::new(profile.token_url.to_string()).map_err(invalid_oauth_url)?)
        .set_redirect_uri(RedirectUrl::new(redirect_uri.to_string()).map_err(invalid_oauth_url)?))
}

fn expires_at_from_duration(
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

fn invalid_oauth_url(error: oauth2::url::ParseError) -> GatewayError {
    GatewayError::Rejected(format!("invalid OAuth provider URL: {error}"))
}

fn oauth_request_error<E>(error: E) -> GatewayError
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gmail_profile_uses_imap_smtp_mail_scope_and_offline_access() {
        let profile = OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile");

        assert_eq!(profile.scopes, &["https://mail.google.com/"]);
        assert!(profile
            .extra_authorization_params
            .contains(&("access_type", "offline")));
    }

    #[test]
    fn outlook_profile_uses_imap_smtp_and_refresh_scopes() {
        let profile = OAuthProviderProfile::for_provider(&ProviderHint::Outlook).expect("profile");

        assert!(profile.scopes.contains(&"offline_access"));
        assert!(profile
            .scopes
            .contains(&"https://outlook.office.com/IMAP.AccessAsUser.All"));
        assert!(profile
            .scopes
            .contains(&"https://outlook.office.com/SMTP.Send"));
    }

    #[test]
    fn token_set_refreshes_inside_expiry_skew() {
        let now = OffsetDateTime::parse("2026-04-27T10:00:00Z", &Rfc3339).expect("now");
        let token_set = OAuthTokenSet {
            r#type: oauth_secret_type(),
            provider: ProviderHint::Gmail,
            client_id: "client".to_string(),
            access_token: "access".to_string(),
            refresh_token: Some("refresh".to_string()),
            expires_at: Some(
                (now + Duration::seconds(OAUTH_REFRESH_SKEW_SECONDS - 1))
                    .format(&Rfc3339)
                    .expect("expiry"),
            ),
            scopes: vec!["https://mail.google.com/".to_string()],
        };

        assert!(token_set.requires_refresh_at(now).expect("refresh check"));
    }

    #[test]
    fn token_set_rejects_wrong_secret_type() {
        let error = OAuthTokenSet::decode(
            r#"{
                "type": "password",
                "provider": "gmail",
                "clientId": "client",
                "accessToken": "access"
            }"#,
        )
        .expect_err("OAuth token secret type is required");

        assert!(
            matches!(error, GatewayError::Rejected(message) if message.contains("secret type"))
        );
    }

    #[test]
    fn authorization_session_uses_pkce_and_state() {
        let service = OAuthTokenService::new().expect("service");
        let profile = OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile");

        let session = service
            .authorization_session(
                &profile,
                "client-id",
                "http://127.0.0.1:12345/oauth/callback",
            )
            .expect("session");

        assert!(session
            .authorization_url
            .contains("code_challenge_method=S256"));
        assert!(session.authorization_url.contains("access_type=offline"));
        assert!(!session.state.is_empty());
        assert!(!session.pkce_verifier.is_empty());
    }
}
