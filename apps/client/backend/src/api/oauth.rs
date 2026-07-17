//! The OAuth authorization flow behind the accounts family: PKCE session
//! minting for the `oauthStart` query and the provider code exchange behind
//! the `completeOauth` command.
//!
//! The flow follows the OAuth 2.1 security posture: authorization code only,
//! PKCE required, no password or implicit grant. The client id / client
//! secret are the app's bundled OAuth registration, not account secret
//! material; the token set the exchange produces is stored through the
//! secret store and never surfaces on the wire.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex as StdMutex;
use std::time::Duration as StdDuration;

use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use oauth2::{
    AuthType, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, ExtraTokenFields,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use posthaste_client_models::{OauthStartQuery, OauthStartResult};
use posthaste_domain_model::{
    GatewayError, ImapTransportSettings, ProviderHint, ProviderKind, ProviderProfile, ServiceError,
    SmtpTransportSettings,
};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use super::ApiFailure;

/// Total wall-clock deadline for a single identity-provider HTTP request, so
/// a hung provider can never wedge a command handler.
const OAUTH_HTTP_TOTAL_TIMEOUT: StdDuration = StdDuration::from_secs(30);

/// TCP + TLS connect deadline for the same shared client.
const OAUTH_HTTP_CONNECT_TIMEOUT: StdDuration = StdDuration::from_secs(10);

/// A minted authorization state is honored this long; after that the flow
/// entry is pruned and the callback is rejected.
const FLOW_TTL_SECONDS: i64 = 10 * 60;

/// Hard cap on concurrently pending authorization flows. The start query is
/// session-token-authenticated (unlike the legacy server's public start
/// endpoint), so this only bounds a runaway local client; the oldest entry
/// is evicted once the prune leaves the map still at capacity.
const FLOW_CAP: usize = 64;

/// JWKS cache lifetime bounds: the provider's `max-age` is honored within
/// them, and the last-good key set is served on a fetch failure.
const JWKS_DEFAULT_CACHE_SECONDS: i64 = 3600;
const JWKS_MAX_CACHE_SECONDS: i64 = 86_400;

/// The one process-wide OAuth HTTP client: bounded timeouts, redirects
/// disabled (redirect URIs are loopback).
static OAUTH_HTTP_CLIENT: LazyLock<oauth2::reqwest::Client> = LazyLock::new(|| {
    oauth2::reqwest::ClientBuilder::new()
        .redirect(oauth2::reqwest::redirect::Policy::none())
        .timeout(OAUTH_HTTP_TOTAL_TIMEOUT)
        .connect_timeout(OAUTH_HTTP_CONNECT_TIMEOUT)
        .build()
        .expect("OAuth HTTP client must build")
});

type OauthTokenResponse =
    oauth2::StandardTokenResponse<OpenIdExtraTokenFields, oauth2::basic::BasicTokenType>;

type OauthClient = oauth2::Client<
    oauth2::StandardErrorResponse<oauth2::basic::BasicErrorResponseType>,
    OauthTokenResponse,
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

/// OAuth 2.0 endpoints and mail scopes for a provider that supports the
/// built-in flow. Eligibility and OIDC issuer policy come from the domain
/// provider profile; the URLs and scopes are this adapter's knowledge.
struct ProviderEndpoints {
    provider: ProviderHint,
    auth_url: &'static str,
    token_url: &'static str,
    metadata_url: &'static str,
    scopes: &'static [&'static str],
    extra_authorization_params: &'static [(&'static str, &'static str)],
}

fn provider_endpoints(provider: &ProviderHint) -> Option<ProviderEndpoints> {
    let profile = ProviderProfile::from_hint(provider);
    if !profile.oauth().is_supported() {
        return None;
    }
    match profile.kind() {
        ProviderKind::Gmail => Some(ProviderEndpoints {
            provider: provider.clone(),
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
            token_url: "https://oauth2.googleapis.com/token",
            metadata_url: "https://accounts.google.com/.well-known/openid-configuration",
            scopes: &["openid", "email", "https://mail.google.com/"],
            extra_authorization_params: &[("access_type", "offline"), ("prompt", "consent")],
        }),
        ProviderKind::Outlook => Some(ProviderEndpoints {
            provider: provider.clone(),
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            metadata_url:
                "https://login.microsoftonline.com/common/v2.0/.well-known/openid-configuration",
            scopes: &[
                "openid",
                "email",
                "offline_access",
                "https://outlook.office.com/IMAP.AccessAsUser.All",
                "https://outlook.office.com/SMTP.Send",
            ],
            extra_authorization_params: &[],
        }),
        ProviderKind::Generic | ProviderKind::Icloud => None,
    }
}

/// One authorization attempt awaiting its callback. Holds the PKCE verifier
/// and nonce, so it is kept in process memory only and never serialized.
struct PendingFlow {
    provider: ProviderHint,
    client_id: String,
    client_secret: Option<String>,
    redirect_uri: String,
    pkce_verifier: String,
    nonce: String,
    started_at: OffsetDateTime,
}

/// Pending flows keyed by their CSRF state. Process-scoped on purpose: an
/// authorization that outlives the backend run must be restarted.
static PENDING_FLOWS: LazyLock<StdMutex<HashMap<String, PendingFlow>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

fn prune_flows(flows: &mut HashMap<String, PendingFlow>, now: OffsetDateTime) {
    flows.retain(|_, flow| now - flow.started_at < Duration::seconds(FLOW_TTL_SECONDS));
    if flows.len() >= FLOW_CAP {
        if let Some(oldest) = flows
            .iter()
            .min_by_key(|(_, flow)| flow.started_at)
            .map(|(state, _)| state.clone())
        {
            flows.remove(&oldest);
        }
    }
}

/// Mint one PKCE authorization session for the `oauthStart` query: build the
/// provider authorization URL, remember the verifier + nonce under the CSRF
/// state, and hand the descriptor back for the client to open a browser with.
pub(crate) fn start_flow(query: &OauthStartQuery) -> Result<OauthStartResult, ApiFailure> {
    let endpoints = provider_endpoints(&query.provider).ok_or_else(|| {
        ApiFailure::malformed("provider does not support the built-in OAuth flow")
    })?;
    let client_id = query.client_id.trim();
    if client_id.is_empty() {
        return Err(ApiFailure::malformed("clientId must not be empty"));
    }
    let redirect_uri = query.redirect_uri.trim();
    if redirect_uri.is_empty() {
        return Err(ApiFailure::malformed("redirectUri must not be empty"));
    }
    let client_secret = query
        .client_secret
        .as_deref()
        .map(str::trim)
        .filter(|secret| !secret.is_empty());

    let client = oauth_client(&endpoints, client_id, client_secret, redirect_uri)?;
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let nonce = CsrfToken::new_random();
    let mut request = client
        .authorize_url(CsrfToken::new_random)
        .set_pkce_challenge(pkce_challenge);
    for scope in endpoints.scopes {
        request = request.add_scope(Scope::new((*scope).to_string()));
    }
    for (name, value) in endpoints.extra_authorization_params {
        request = request.add_extra_param(*name, *value);
    }
    request = request.add_extra_param("nonce", nonce.secret().clone());
    let (authorization_url, state) = request.url();

    let now = OffsetDateTime::now_utc();
    let mut flows = PENDING_FLOWS.lock().expect("OAuth flow map poisoned");
    prune_flows(&mut flows, now);
    flows.insert(
        state.secret().clone(),
        PendingFlow {
            provider: endpoints.provider,
            client_id: client_id.to_string(),
            client_secret: client_secret.map(ToString::to_string),
            redirect_uri: redirect_uri.to_string(),
            pkce_verifier: pkce_verifier.secret().clone(),
            nonce: nonce.secret().clone(),
            started_at: now,
        },
    );

    Ok(OauthStartResult {
        authorization_url: authorization_url.to_string(),
        state: state.secret().clone(),
        redirect_uri: redirect_uri.to_string(),
    })
}

/// Everything the accounts family needs to create an account from a finished
/// authorization: the verified identity, the provider mail endpoints, and
/// the encoded token set destined for the secret store.
pub(crate) struct OauthAccountSeed {
    pub(crate) provider: ProviderHint,
    pub(crate) identity_email: String,
    /// The token-set JSON to store as the account secret. Secret material:
    /// hand it straight to the secret store, keep it out of logs and errors.
    pub(crate) token_set_json: String,
    pub(crate) imap: ImapTransportSettings,
    pub(crate) smtp: SmtpTransportSettings,
}

/// Complete an authorization the `oauthStart` query began: take the pending
/// flow (single-use — a replayed `completeOauth` command is answered by the
/// command-outcome cache, not by re-running the exchange), exchange the code
/// with the provider, and verify the OIDC identity.
pub(crate) async fn complete_flow(state: &str, code: &str) -> Result<OauthAccountSeed, ApiFailure> {
    let code = code.trim();
    if code.is_empty() {
        return Err(ApiFailure::malformed(
            "authorization code must not be empty",
        ));
    }
    let flow = {
        let now = OffsetDateTime::now_utc();
        let mut flows = PENDING_FLOWS.lock().expect("OAuth flow map poisoned");
        prune_flows(&mut flows, now);
        flows.remove(state)
    }
    .ok_or_else(|| ApiFailure::malformed("OAuth state is unknown, expired, or already used"))?;

    let endpoints = provider_endpoints(&flow.provider).ok_or_else(|| {
        ApiFailure::malformed("provider does not support the built-in OAuth flow")
    })?;
    let client = oauth_client(
        &endpoints,
        &flow.client_id,
        flow.client_secret.as_deref(),
        &flow.redirect_uri,
    )?;
    let now = OffsetDateTime::now_utc();
    let token_response = client
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .set_pkce_verifier(PkceCodeVerifier::new(flow.pkce_verifier.clone()))
        .request_async(&*OAUTH_HTTP_CLIENT)
        .await
        .map_err(|error| service_failure(oauth_request_error(error)))?;

    let identity_email = verified_identity_email(
        &endpoints,
        &flow.client_id,
        &token_response,
        &flow.nonce,
        now,
    )
    .await
    .map_err(service_failure)?;

    let (imap, smtp) = ProviderProfile::from_hint(&flow.provider)
        .oauth()
        .default_mail_transport()
        .ok_or_else(|| {
            ApiFailure::malformed("provider does not support built-in OAuth account creation")
        })?;

    let token_set = OauthTokenSet {
        r#type: "oauth2".to_string(),
        provider: flow.provider.clone(),
        client_id: flow.client_id,
        client_secret: flow.client_secret,
        access_token: token_response.access_token().secret().clone(),
        refresh_token: token_response
            .refresh_token()
            .map(|token| token.secret().clone()),
        expires_at: expires_at_from_duration(now, token_response.expires_in())
            .map_err(service_failure)?,
        scopes: token_response
            .scopes()
            .map(|scopes| scopes.iter().map(|scope| scope.to_string()).collect())
            .unwrap_or_else(|| {
                endpoints
                    .scopes
                    .iter()
                    .map(|scope| (*scope).to_string())
                    .collect()
            }),
    };
    let token_set_json = serde_json::to_string(&token_set)
        .map_err(|_| ApiFailure::internal("failed to encode the OAuth token set"))?;

    Ok(OauthAccountSeed {
        provider: flow.provider,
        identity_email,
        token_set_json,
        imap,
        smtp,
    })
}

fn service_failure(error: GatewayError) -> ApiFailure {
    ApiFailure::from(ServiceError::from(error))
}

fn oauth_client(
    endpoints: &ProviderEndpoints,
    client_id: &str,
    client_secret: Option<&str>,
    redirect_uri: &str,
) -> Result<OauthClient, ApiFailure> {
    let invalid_url = |error: oauth2::url::ParseError| {
        ApiFailure::malformed(format!("invalid OAuth URL: {error}"))
    };
    let mut client = oauth2::Client::new(ClientId::new(client_id.to_string()))
        .set_auth_uri(AuthUrl::new(endpoints.auth_url.to_string()).map_err(invalid_url)?)
        .set_token_uri(TokenUrl::new(endpoints.token_url.to_string()).map_err(invalid_url)?)
        .set_redirect_uri(RedirectUrl::new(redirect_uri.to_string()).map_err(invalid_url)?);
    if let Some(client_secret) = client_secret {
        client = client
            .set_client_secret(ClientSecret::new(client_secret.to_string()))
            .set_auth_type(AuthType::RequestBody);
    }
    Ok(client)
}

fn oauth_request_error<E: std::fmt::Display>(error: E) -> GatewayError {
    let message = error.to_string();
    if message.contains("invalid_grant") || message.contains("unauthorized_client") {
        GatewayError::Auth
    } else {
        // The provider/transport error Display can carry the provider's
        // response body and request URL; log it server-side and return a
        // fixed message so unbounded third-party text never reaches the client.
        tracing::warn!(%message, "OAuth token-endpoint request failed");
        GatewayError::Network("the OAuth provider could not be reached".to_string())
    }
}

fn invalid_openid_token<E: std::fmt::Display>(error: E) -> GatewayError {
    GatewayError::Rejected(format!("OAuth identity token is invalid: {error}"))
}

fn expires_at_from_duration(
    now: OffsetDateTime,
    expires_in: Option<StdDuration>,
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

/// The stored account secret for an OAuth account: the whole token bundle,
/// JSON-encoded, so a refresh can mint new access tokens without
/// re-authorizing. Never returned by any API answer.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OauthTokenSet {
    r#type: String,
    provider: ProviderHint,
    client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_secret: Option<String>,
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<String>,
    scopes: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct OpenIdExtraTokenFields {
    #[serde(default)]
    id_token: Option<String>,
}

impl ExtraTokenFields for OpenIdExtraTokenFields {}

#[derive(Deserialize)]
struct OpenIdTokenClaims {
    aud: Option<OpenIdAudience>,
    email: Option<String>,
    email_verified: Option<bool>,
    exp: Option<i64>,
    nbf: Option<i64>,
    iss: Option<String>,
    preferred_username: Option<String>,
    upn: Option<String>,
    nonce: Option<String>,
}

#[derive(Deserialize)]
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

/// Extract and verify the OIDC identity email from the token response: the
/// id_token signature is checked against the provider's JWKS, and the
/// audience, issuer, expiry, and nonce claims are validated.
async fn verified_identity_email(
    endpoints: &ProviderEndpoints,
    client_id: &str,
    token_response: &OauthTokenResponse,
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

    let jwks = jwks_for(endpoints.metadata_url, now, false).await?;
    let claims = match decode_verified_claims(id_token, kid, &jwks) {
        Ok(claims) => claims,
        // A rotated signing key: refresh the JWKS once and retry.
        Err(GatewayError::Rejected(ref message)) if message.contains("signing key") => {
            let refreshed = jwks_for(endpoints.metadata_url, now, true).await?;
            decode_verified_claims(id_token, kid, &refreshed)?
        }
        Err(error) => return Err(error),
    };
    validate_identity_claims(endpoints, client_id, &claims, expected_nonce, now)?;

    claims
        .email
        .or(claims.preferred_username)
        .or(claims.upn)
        .map(|email| email.trim().to_string())
        .filter(|email| email.contains('@'))
        .ok_or_else(|| {
            GatewayError::Rejected("OAuth identity did not include an email address".to_string())
        })
}

fn decode_verified_claims(
    id_token: &str,
    kid: &str,
    jwks: &JwkSet,
) -> Result<OpenIdTokenClaims, GatewayError> {
    let jwk = jwks.find(kid).ok_or_else(|| {
        GatewayError::Rejected("OAuth identity token signing key was not found".to_string())
    })?;
    let decoding_key = DecodingKey::from_jwk(jwk).map_err(invalid_openid_token)?;
    // Claim validation (aud/iss/exp/nbf/nonce) happens explicitly in
    // `validate_identity_claims` so its failures are typed; the library pass
    // verifies only the signature.
    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_exp = false;
    validation.validate_nbf = false;
    validation.validate_aud = false;
    validation.required_spec_claims.clear();
    let token_data = decode::<OpenIdTokenClaims>(id_token, &decoding_key, &validation)
        .map_err(invalid_openid_token)?;
    Ok(token_data.claims)
}

/// Clock-skew leeway for the `nbf` (not-before) claim, in seconds.
const OPENID_NBF_LEEWAY_SECONDS: i64 = 60;

fn validate_identity_claims(
    endpoints: &ProviderEndpoints,
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
    let issuer_matches = claims.iss.as_deref().is_some_and(|issuer| {
        ProviderProfile::from_hint(&endpoints.provider)
            .oauth()
            .openid_issuer_matches(issuer)
    });
    if !issuer_matches {
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
    if let Some(not_before) = claims.nbf {
        let not_before = OffsetDateTime::from_unix_timestamp(not_before).map_err(|error| {
            GatewayError::Rejected(format!("OAuth identity token nbf is invalid: {error}"))
        })?;
        if now + Duration::seconds(OPENID_NBF_LEEWAY_SECONDS) < not_before {
            return Err(GatewayError::Rejected(
                "OAuth identity token is not yet valid".to_string(),
            ));
        }
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

struct CachedJwks {
    jwks: JwkSet,
    expires_at: OffsetDateTime,
}

/// Last-good JWKS per provider metadata URL. The lock is held across the
/// fetch, serializing concurrent exchanges against one provider — at most a
/// handful ever run on this single-user backend, and serializing them is
/// what keeps a burst from stampeding the provider.
static JWKS_CACHE: LazyLock<tokio::sync::Mutex<HashMap<&'static str, CachedJwks>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));

async fn jwks_for(
    metadata_url: &'static str,
    now: OffsetDateTime,
    force_refresh: bool,
) -> Result<JwkSet, GatewayError> {
    let mut cache = JWKS_CACHE.lock().await;
    if !force_refresh {
        if let Some(cached) = cache.get(metadata_url) {
            if cached.expires_at > now {
                return Ok(cached.jwks.clone());
            }
        }
    }
    match fetch_jwks(metadata_url, now).await {
        Ok(fetched) => {
            let jwks = fetched.jwks.clone();
            cache.insert(metadata_url, fetched);
            Ok(jwks)
        }
        // A fetch failure with a last-good key set degrades to the stale
        // keys; a cold cache propagates the fetch error.
        Err(error) => cache
            .get(metadata_url)
            .map(|cached| cached.jwks.clone())
            .ok_or(error),
    }
}

#[derive(Deserialize)]
struct OpenIdProviderMetadata {
    jwks_uri: String,
}

async fn fetch_jwks(
    metadata_url: &'static str,
    now: OffsetDateTime,
) -> Result<CachedJwks, GatewayError> {
    let metadata = OAUTH_HTTP_CLIENT
        .get(metadata_url)
        .send()
        .await
        .map_err(oauth_request_error)?;
    if !metadata.status().is_success() {
        return Err(GatewayError::Network(format!(
            "OAuth metadata request failed with {}",
            metadata.status()
        )));
    }
    let metadata_body = metadata.text().await.map_err(oauth_request_error)?;
    let metadata: OpenIdProviderMetadata =
        serde_json::from_str(&metadata_body).map_err(oauth_request_error)?;

    let jwks_response = OAUTH_HTTP_CLIENT
        .get(&metadata.jwks_uri)
        .send()
        .await
        .map_err(oauth_request_error)?;
    if !jwks_response.status().is_success() {
        return Err(GatewayError::Network(format!(
            "OAuth JWKS request failed with {}",
            jwks_response.status()
        )));
    }
    let expires_at = now + jwks_cache_duration(jwks_response.headers());
    let jwks_body = jwks_response.text().await.map_err(oauth_request_error)?;
    let jwks = serde_json::from_str(&jwks_body).map_err(oauth_request_error)?;
    Ok(CachedJwks { jwks, expires_at })
}

fn jwks_cache_duration(headers: &oauth2::http::HeaderMap) -> Duration {
    let seconds = headers
        .get(oauth2::http::header::CACHE_CONTROL)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value.split(',').find_map(|directive| {
                directive
                    .trim()
                    .strip_prefix("max-age=")
                    .and_then(|seconds| seconds.parse::<i64>().ok())
            })
        })
        .unwrap_or(JWKS_DEFAULT_CACHE_SECONDS)
        .clamp(1, JWKS_MAX_CACHE_SECONDS);
    Duration::seconds(seconds)
}
