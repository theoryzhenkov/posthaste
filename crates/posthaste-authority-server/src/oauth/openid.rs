use super::*;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct OpenIdExtraTokenFields {
    #[serde(default)]
    pub(crate) id_token: Option<String>,
}

impl ExtraTokenFields for OpenIdExtraTokenFields {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub(crate) struct OpenIdTokenClaims {
    pub(crate) aud: Option<OpenIdAudience>,
    pub(crate) email: Option<String>,
    pub(crate) email_verified: Option<bool>,
    pub(crate) exp: Option<i64>,
    pub(crate) nbf: Option<i64>,
    pub(crate) iss: Option<String>,
    pub(crate) preferred_username: Option<String>,
    pub(crate) upn: Option<String>,
    pub(crate) nonce: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(untagged)]
pub(crate) enum OpenIdAudience {
    One(String),
    Many(Vec<String>),
}

impl OpenIdAudience {
    pub(crate) fn contains(&self, client_id: &str) -> bool {
        match self {
            Self::One(audience) => audience == client_id,
            Self::Many(audiences) => audiences.iter().any(|audience| audience == client_id),
        }
    }
}

#[derive(Clone)]
pub(crate) struct CachedJwks {
    pub(crate) jwks: JwkSet,
    pub(crate) expires_at: OffsetDateTime,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenIdProviderMetadata {
    pub(crate) jwks_uri: String,
}

pub(crate) fn decode_verified_openid_claims(
    profile: &OAuthProviderProfile,
    client_id: &str,
    id_token: &str,
    kid: &str,
    jwks: &JwkSet,
    expected_nonce: &str,
    now: OffsetDateTime,
) -> Result<OpenIdTokenClaims, GatewayError> {
    let jwk = jwks.find(kid).ok_or_else(|| {
        GatewayError::Rejected("OAuth identity token signing key was not found".to_string())
    })?;
    let decoding_key = DecodingKey::from_jwk(jwk).map_err(invalid_openid_token)?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_exp = false;
    validation.validate_nbf = false;
    validation.validate_aud = false;
    validation.required_spec_claims.clear();
    let token_data = decode::<OpenIdTokenClaims>(id_token, &decoding_key, &validation)
        .map_err(invalid_openid_token)?;
    validate_openid_identity_claims(profile, client_id, &token_data.claims, expected_nonce, now)?;
    Ok(token_data.claims)
}

pub(crate) fn jwks_cache_duration(headers: &oauth2::http::HeaderMap) -> Duration {
    let seconds = headers
        .get(oauth2::http::header::CACHE_CONTROL)
        .and_then(|value| value.to_str().ok())
        .and_then(cache_control_max_age)
        .unwrap_or(OAUTH_JWKS_DEFAULT_CACHE_SECONDS)
        .clamp(1, OAUTH_JWKS_MAX_CACHE_SECONDS);
    Duration::seconds(seconds)
}

pub(crate) fn cache_control_max_age(value: &str) -> Option<i64> {
    value.split(',').find_map(|directive| {
        directive
            .trim()
            .strip_prefix("max-age=")
            .and_then(|seconds| seconds.parse::<i64>().ok())
    })
}

#[cfg(test)]
pub(crate) fn insecure_openid_claims_from_id_token(
    id_token: &str,
) -> Result<OpenIdTokenClaims, GatewayError> {
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

/// Clock-skew leeway for the `nbf` (not-before) claim, in seconds.
pub(crate) const OPENID_NBF_LEEWAY_SECONDS: i64 = 60;

pub(crate) fn validate_openid_identity_claims(
    profile: &OAuthProviderProfile,
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
        .is_some_and(|issuer| profile.openid_issuer_matches(issuer))
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
    if let Some(not_before) = claims.nbf {
        let not_before = OffsetDateTime::from_unix_timestamp(not_before).map_err(|error| {
            GatewayError::Rejected(format!("OAuth identity token nbf is invalid: {error}"))
        })?;
        // Allow a small clock-skew leeway so a freshly-issued token isn't rejected.
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
