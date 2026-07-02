use super::*;

/// Serializable OAuth token bundle stored as the account secret value.
///
/// The API never returns this payload. It is resolved only inside the authority server
/// and converted to a short-lived access token before opening XOAUTH2 sessions.
///
/// @spec docs/L1-api#secret-management
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthTokenSet {
    #[serde(default = "oauth_secret_type")]
    pub r#type: String,
    pub provider: ProviderHint,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl fmt::Debug for OAuthTokenSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthTokenSet")
            .field("type", &self.r#type)
            .field("provider", &self.provider)
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[redacted]"),
            )
            .field("access_token", &"[redacted]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[redacted]"),
            )
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .finish()
    }
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

pub(crate) fn oauth_secret_type() -> String {
    "oauth2".to_string()
}
