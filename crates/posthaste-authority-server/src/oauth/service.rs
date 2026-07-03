use super::*;
use std::time::Duration as StdDuration;

/// Total wall-clock deadline for a single IdP HTTP request. Token exchange,
/// refresh, OIDC discovery, and JWKS all inherit it from the one shared client
/// below, so a hung IdP can never wedge a callback handler or the supervisor
/// refresh path (D65, audit N11). REVIEW: 30s total, flagged for owner review.
pub(crate) const OAUTH_HTTP_TOTAL_TIMEOUT: StdDuration = StdDuration::from_secs(30);
/// TCP + TLS connect deadline for the same shared client.
/// REVIEW: 10s connect, flagged for owner review.
const OAUTH_HTTP_CONNECT_TIMEOUT: StdDuration = StdDuration::from_secs(10);

/// The one process-wide OAuth HTTP client. Built exactly once with the timeouts
/// above and shared by every IdP call via a cheap `Arc` clone, replacing the
/// per-request client rebuilds at the `/v1/oauth/*` handlers and the supervisor
/// refresh path. Redirects stay disabled (loopback-only redirect URIs).
static SHARED_OAUTH_HTTP_CLIENT: LazyLock<oauth2::reqwest::Client> = LazyLock::new(|| {
    oauth2::reqwest::ClientBuilder::new()
        .redirect(oauth2::reqwest::redirect::Policy::none())
        .timeout(OAUTH_HTTP_TOTAL_TIMEOUT)
        .connect_timeout(OAUTH_HTTP_CONNECT_TIMEOUT)
        .build()
        .expect("shared OAuth HTTP client (rustls-tls) must build")
});

#[derive(Clone)]
pub struct OAuthTokenService {
    pub(super) http_client: oauth2::reqwest::Client,
}

pub struct OAuthAuthorizationCodeExchange<'a> {
    pub profile: &'a OAuthProviderProfile,
    pub client_id: &'a str,
    pub client_secret: Option<&'a str>,
    pub redirect_uri: &'a str,
    pub code: &'a str,
    pub pkce_verifier: &'a str,
    pub nonce: &'a str,
    pub now: OffsetDateTime,
}

impl OAuthTokenService {
    pub fn new() -> Result<Self, GatewayError> {
        // Clones the process-wide timed client (Arc-backed, cheap); no per-request
        // client is ever rebuilt. `Result` is retained for call-site compatibility.
        Ok(Self {
            http_client: SHARED_OAUTH_HTTP_CLIENT.clone(),
        })
    }

    pub fn authorization_session(
        &self,
        profile: &OAuthProviderProfile,
        client_id: &str,
        client_secret: Option<&str>,
        redirect_uri: &str,
    ) -> Result<OAuthAuthorizationSession, GatewayError> {
        let client = oauth_client(profile, client_id, client_secret, redirect_uri)?;
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
        request = request.add_extra_param("nonce", nonce.secret().clone());

        let (authorization_url, state) = request.url();
        Ok(OAuthAuthorizationSession {
            authorization_url: authorization_url.to_string(),
            state: state.secret().clone(),
            pkce_verifier: pkce_verifier.secret().clone(),
            nonce: nonce.secret().clone(),
            redirect_uri: redirect_uri.to_string(),
        })
    }

    pub async fn exchange_authorization_code(
        &self,
        request: OAuthAuthorizationCodeExchange<'_>,
    ) -> Result<OAuthExchangeResult, GatewayError> {
        let OAuthAuthorizationCodeExchange {
            profile,
            client_id,
            client_secret,
            redirect_uri,
            code,
            pkce_verifier,
            nonce,
            now,
        } = request;
        let client = oauth_client(profile, client_id, client_secret, redirect_uri)?;
        let token_response = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier.to_string()))
            .request_async(&self.http_client)
            .await
            .map_err(oauth_request_error)?;
        let identity_email = self
            .oauth_identity_email(profile, client_id, &token_response, nonce, now)
            .await?;

        Ok(OAuthExchangeResult {
            identity_email,
            token_set: OAuthTokenSet {
                r#type: oauth_secret_type(),
                provider: profile.provider.clone(),
                client_id: client_id.to_string(),
                client_secret: client_secret.map(ToString::to_string),
                access_token: token_response.access_token().secret().clone(),
                refresh_token: token_response
                    .refresh_token()
                    .map(|token| token.secret().clone()),
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
        let client = oauth_client(
            &profile,
            &token_set.client_id,
            token_set.client_secret.as_deref(),
            "http://127.0.0.1/unused",
        )?;
        let token_response = client
            .exchange_refresh_token(&RefreshToken::new(refresh_token.clone()))
            .request_async(&self.http_client)
            .await
            .map_err(oauth_request_error)?;

        let updated = OAuthTokenSet {
            r#type: oauth_secret_type(),
            provider: token_set.provider.clone(),
            client_id: token_set.client_id.clone(),
            client_secret: token_set.client_secret.clone(),
            access_token: token_response.access_token().secret().clone(),
            refresh_token: token_response
                .refresh_token()
                .map(|token| token.secret().clone())
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

    async fn oauth_identity_email(
        &self,
        profile: &OAuthProviderProfile,
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
        let claims = self
            .verified_openid_claims(profile, client_id, id_token, expected_nonce, now)
            .await?;

        let email = claims
            .email
            .or(claims.preferred_username)
            .or(claims.upn)
            .map(|email| email.trim().to_string())
            .filter(|email| email.contains('@'))
            .ok_or_else(|| {
                GatewayError::Rejected(
                    "OAuth identity did not include an email address".to_string(),
                )
            })?;
        Ok(email)
    }

    async fn verified_openid_claims(
        &self,
        profile: &OAuthProviderProfile,
        client_id: &str,
        id_token: &str,
        expected_nonce: &str,
        now: OffsetDateTime,
    ) -> Result<OpenIdTokenClaims, GatewayError> {
        let header = decode_header(id_token).map_err(invalid_openid_token)?;
        if header.alg != Algorithm::RS256 {
            return Err(GatewayError::Rejected(format!(
                "OAuth identity token algorithm is not supported: {:?}",
                header.alg
            )));
        }
        let kid = header.kid.as_deref().ok_or_else(|| {
            GatewayError::Rejected("OAuth identity token is missing key id".to_string())
        })?;

        let cached_jwks = self.jwks_for_profile(profile, now, false).await?;
        match decode_verified_openid_claims(
            profile,
            client_id,
            id_token,
            kid,
            &cached_jwks,
            expected_nonce,
            now,
        ) {
            Ok(claims) => Ok(claims),
            Err(GatewayError::Rejected(ref message)) if message.contains("signing key") => {
                let refreshed_jwks = self.jwks_for_profile(profile, now, true).await?;
                decode_verified_openid_claims(
                    profile,
                    client_id,
                    id_token,
                    kid,
                    &refreshed_jwks,
                    expected_nonce,
                    now,
                )
            }
            Err(error) => Err(error),
        }
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
