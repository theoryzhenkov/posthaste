use std::collections::HashMap;

use base64::Engine;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, CsrfToken, ExtraTokenFields, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use posthaste_domain::{GatewayError, ProviderHint};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};
use tokio::sync::Mutex;

const OAUTH_REFRESH_SKEW_SECONDS: i64 = 300;

type OAuthTokenResponse =
    oauth2::StandardTokenResponse<OpenIdExtraTokenFields, oauth2::basic::BasicTokenType>;

type OAuthClient = oauth2::Client<
    oauth2::StandardErrorResponse<oauth2::basic::BasicErrorResponseType>,
    OAuthTokenResponse,
    oauth2::StandardTokenIntrospectionResponse<
        OpenIdExtraTokenFields,
        oauth2::basic::BasicTokenType,
    >,
    oauth2::StandardRevocableToken,
    oauth2::StandardErrorResponse<oauth2::RevocationErrorResponseType>,
    oauth2::EndpointSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointNotSet,
    oauth2::EndpointSet,
>;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct OpenIdExtraTokenFields {
    #[serde(default)]
    id_token: Option<String>,
}

impl ExtraTokenFields for OpenIdExtraTokenFields {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct OpenIdTokenClaims {
    aud: Option<OpenIdAudience>,
    email: Option<String>,
    email_verified: Option<bool>,
    exp: Option<i64>,
    iss: Option<String>,
    preferred_username: Option<String>,
    upn: Option<String>,
    nonce: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(untagged)]
enum OpenIdAudience {
    One(String),
    Many(Vec<String>),
}

impl OpenIdAudience {
    fn contains(&self, client_id: &str) -> bool {
        match self {
            Self::One(audience) => audience == client_id,
            Self::Many(audiences) => audiences.iter().any(|audience| audience == client_id),
        }
    }
}

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
                scopes: &["openid", "email", "https://mail.google.com/"],
                extra_authorization_params: &[("access_type", "offline"), ("prompt", "consent")],
            }),
            ProviderHint::Outlook => Some(Self {
                provider: ProviderHint::Outlook,
                auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
                token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
                scopes: &[
                    "openid",
                    "email",
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
    #[serde(skip_serializing)]
    pub pkce_verifier: String,
    #[serde(skip_serializing)]
    pub nonce: String,
    pub redirect_uri: String,
}

#[derive(Clone, Debug)]
pub struct PendingOAuthFlow {
    pub account_id: Option<posthaste_domain::AccountId>,
    pub profile: OAuthProviderProfile,
    pub client_id: String,
    pub redirect_uri: String,
    pub pkce_verifier: String,
    pub nonce: String,
}

#[derive(Default)]
pub struct OAuthFlowStore {
    flows: Mutex<HashMap<String, PendingOAuthFlow>>,
}

impl OAuthFlowStore {
    pub async fn insert(&self, state: String, flow: PendingOAuthFlow) {
        self.flows.lock().await.insert(state, flow);
    }

    pub async fn remove(&self, state: &str) -> Option<PendingOAuthFlow> {
        self.flows.lock().await.remove(state)
    }
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
        let nonce = CsrfToken::new_random();
        let mut request = client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge);

        for scope in profile.scopes {
            request = request.add_scope(Scope::new((*scope).to_string()));
        }
        for (name, value) in profile.extra_authorization_params {
            request = request.add_extra_param(*name, *value);
        }
        request = request.add_extra_param("nonce", nonce.secret().to_string());

        let (authorization_url, state) = request.url();
        Ok(OAuthAuthorizationSession {
            authorization_url: authorization_url.to_string(),
            state: state.secret().to_string(),
            pkce_verifier: pkce_verifier.secret().to_string(),
            nonce: nonce.secret().to_string(),
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
        nonce: &str,
        now: OffsetDateTime,
    ) -> Result<OAuthExchangeResult, GatewayError> {
        let client = oauth_client(profile, client_id, redirect_uri)?;
        let token_response = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier.to_string()))
            .request_async(&self.http_client)
            .await
            .map_err(oauth_request_error)?;
        let identity_email =
            oauth_identity_email(&profile.provider, client_id, &token_response, nonce, now)?;

        Ok(OAuthExchangeResult {
            identity_email,
            token_set: OAuthTokenSet {
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
            },
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

pub struct OAuthExchangeResult {
    pub token_set: OAuthTokenSet,
    pub identity_email: String,
}

pub struct OAuthAccessToken {
    pub token: String,
    pub updated_token_set: Option<OAuthTokenSet>,
}

fn oauth_client(
    profile: &OAuthProviderProfile,
    client_id: &str,
    redirect_uri: &str,
) -> Result<OAuthClient, GatewayError> {
    Ok(oauth2::Client::new(ClientId::new(client_id.to_string()))
        .set_auth_uri(AuthUrl::new(profile.auth_url.to_string()).map_err(invalid_oauth_url)?)
        .set_token_uri(TokenUrl::new(profile.token_url.to_string()).map_err(invalid_oauth_url)?)
        .set_redirect_uri(RedirectUrl::new(redirect_uri.to_string()).map_err(invalid_oauth_url)?))
}

fn oauth_identity_email(
    provider: &ProviderHint,
    client_id: &str,
    token_response: &OAuthTokenResponse,
    expected_nonce: &str,
    now: OffsetDateTime,
) -> Result<String, GatewayError> {
    let id_token = token_response
        .extra_fields()
        .id_token
        .as_deref()
        .ok_or_else(|| {
            GatewayError::Rejected("OAuth response did not include id_token".to_string())
        })?;
    let claims = openid_claims_from_id_token(id_token)?;
    validate_openid_identity_claims(provider, client_id, &claims, expected_nonce, now)?;

    let email = claims
        .email
        .or(claims.preferred_username)
        .or(claims.upn)
        .map(|email| email.trim().to_string())
        .filter(|email| email.contains('@'))
        .ok_or_else(|| {
            GatewayError::Rejected("OAuth identity did not include an email address".to_string())
        })?;
    Ok(email)
}

fn validate_openid_identity_claims(
    provider: &ProviderHint,
    client_id: &str,
    claims: &OpenIdTokenClaims,
    expected_nonce: &str,
    now: OffsetDateTime,
) -> Result<(), GatewayError> {
    if !claims
        .aud
        .as_ref()
        .is_some_and(|audience| audience.contains(client_id))
    {
        return Err(GatewayError::Rejected(
            "OAuth identity token audience did not match".to_string(),
        ));
    }
    if !claims
        .iss
        .as_deref()
        .is_some_and(|issuer| openid_issuer_matches(provider, issuer))
    {
        return Err(GatewayError::Rejected(
            "OAuth identity token issuer did not match".to_string(),
        ));
    }
    let expires_at = claims.exp.ok_or_else(|| {
        GatewayError::Rejected("OAuth identity token expiry is missing".to_string())
    })?;
    let expires_at = OffsetDateTime::from_unix_timestamp(expires_at).map_err(|error| {
        GatewayError::Rejected(format!("OAuth identity token expiry is invalid: {error}"))
    })?;
    if expires_at <= now {
        return Err(GatewayError::Rejected(
            "OAuth identity token has expired".to_string(),
        ));
    }
    if claims.nonce.as_deref() != Some(expected_nonce) {
        return Err(GatewayError::Rejected(
            "OAuth identity token nonce did not match".to_string(),
        ));
    }
    if claims.email_verified == Some(false) {
        return Err(GatewayError::Rejected(
            "OAuth identity email is not verified".to_string(),
        ));
    }
    Ok(())
}

fn openid_issuer_matches(provider: &ProviderHint, issuer: &str) -> bool {
    match provider {
        ProviderHint::Gmail => {
            issuer == "https://accounts.google.com" || issuer == "accounts.google.com"
        }
        ProviderHint::Outlook => {
            issuer.starts_with("https://login.microsoftonline.com/") && issuer.ends_with("/v2.0")
        }
        ProviderHint::Generic | ProviderHint::Icloud => false,
    }
}

fn openid_claims_from_id_token(id_token: &str) -> Result<OpenIdTokenClaims, GatewayError> {
    let payload = id_token
        .split('.')
        .nth(1)
        .ok_or_else(|| GatewayError::Rejected("OAuth identity token is not a JWT".to_string()))?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|error| {
            GatewayError::Rejected(format!("OAuth identity token payload is invalid: {error}"))
        })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        GatewayError::Rejected(format!("OAuth identity token claims are invalid: {error}"))
    })
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

        assert_eq!(
            profile.scopes,
            &["openid", "email", "https://mail.google.com/"]
        );
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
        assert!(session.authorization_url.contains("nonce="));
        assert!(!session.state.is_empty());
        assert!(!session.pkce_verifier.is_empty());
        assert!(!session.nonce.is_empty());
    }

    #[test]
    fn openid_claims_require_matching_nonce_and_verified_email() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            serde_json::json!({
                "aud": "client-id",
                "email": "user@example.test",
                "email_verified": true,
                "exp": 2000000000,
                "iss": "https://accounts.google.com",
                "nonce": "expected-nonce",
            })
            .to_string(),
        );
        let claims =
            openid_claims_from_id_token(&format!("header.{payload}.signature")).expect("claims");

        assert_eq!(claims.email.as_deref(), Some("user@example.test"));
        assert_eq!(claims.email_verified, Some(true));
        assert_eq!(claims.nonce.as_deref(), Some("expected-nonce"));
        assert!(validate_openid_identity_claims(
            &ProviderHint::Gmail,
            "client-id",
            &claims,
            "expected-nonce",
            OffsetDateTime::parse("2026-04-27T10:00:00Z", &Rfc3339).expect("now"),
        )
        .is_ok());
    }

    #[test]
    fn openid_claim_validation_rejects_wrong_audience() {
        let claims = OpenIdTokenClaims {
            aud: Some(OpenIdAudience::One("other-client".to_string())),
            email: Some("user@example.test".to_string()),
            email_verified: Some(true),
            exp: Some(2_000_000_000),
            iss: Some("https://accounts.google.com".to_string()),
            preferred_username: None,
            upn: None,
            nonce: Some("expected-nonce".to_string()),
        };

        let error = validate_openid_identity_claims(
            &ProviderHint::Gmail,
            "client-id",
            &claims,
            "expected-nonce",
            OffsetDateTime::parse("2026-04-27T10:00:00Z", &Rfc3339).expect("now"),
        )
        .expect_err("wrong audience should be rejected");

        assert!(matches!(error, GatewayError::Rejected(message) if message.contains("audience")));
    }

    #[tokio::test]
    async fn flow_store_removes_pending_state_once() {
        let store = OAuthFlowStore::default();
        let profile = OAuthProviderProfile::for_provider(&ProviderHint::Gmail).expect("profile");
        let flow = PendingOAuthFlow {
            account_id: Some(posthaste_domain::AccountId::from("gmail")),
            profile,
            client_id: "client-id".to_string(),
            redirect_uri: "http://127.0.0.1:12345/v1/oauth/callback".to_string(),
            pkce_verifier: "verifier".to_string(),
            nonce: "nonce".to_string(),
        };

        store.insert("state".to_string(), flow).await;

        assert!(store.remove("state").await.is_some());
        assert!(store.remove("state").await.is_none());
    }
}
