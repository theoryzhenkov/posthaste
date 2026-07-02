use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

#[cfg(test)]
use base64::Engine;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use oauth2::{
    AuthType, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, ExtraTokenFields,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use posthaste_domain_service::{
    GatewayError, ImapTransportSettings, ProviderHint, ProviderKind, ProviderProfile,
    SmtpTransportSettings,
};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};
use tokio::sync::Mutex;

const OAUTH_REFRESH_SKEW_SECONDS: i64 = 300;
const OAUTH_JWKS_DEFAULT_CACHE_SECONDS: i64 = 3600;
const OAUTH_JWKS_MAX_CACHE_SECONDS: i64 = 86_400;

static OAUTH_JWKS_CACHE: OnceLock<Mutex<HashMap<&'static str, CachedJwks>>> = OnceLock::new();

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

mod flow_store;
mod jwks;
mod openid;
mod profile;
mod service;
mod support;
mod token_set;

pub use flow_store::{
    OAuthAuthorizationSession, OAuthFlowCompletion, OAuthFlowStore, PendingOAuthFlow,
};
pub use profile::OAuthProviderProfile;
pub use service::{
    OAuthAccessToken, OAuthAuthorizationCodeExchange, OAuthExchangeResult, OAuthTokenService,
};
pub use token_set::OAuthTokenSet;

pub(crate) use openid::{
    decode_verified_openid_claims, jwks_cache_duration, CachedJwks, OpenIdExtraTokenFields,
    OpenIdProviderMetadata, OpenIdTokenClaims,
};
#[cfg(test)]
pub(crate) use openid::{
    insecure_openid_claims_from_id_token, validate_openid_identity_claims, OpenIdAudience,
    OPENID_NBF_LEEWAY_SECONDS,
};
pub(crate) use support::{
    expires_at_from_duration, invalid_openid_token, oauth_client, oauth_request_error,
};
pub(crate) use token_set::oauth_secret_type;

#[cfg(test)]
mod tests;
