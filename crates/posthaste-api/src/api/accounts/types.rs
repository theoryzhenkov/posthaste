use super::*;

/// Transport fields for account create/patch requests.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountTransportRequest {
    pub provider: Option<ProviderHint>,
    pub auth: Option<ProviderAuthKind>,
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub imap: Option<ImapTransportSettings>,
    pub smtp: Option<SmtpTransportSettings>,
}

/// Tri-state write mode controlling how a secret is mutated on account save.
///
/// @spec docs/L1-api#secret-management
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum SecretWriteMode {
    #[default]
    Keep,
    Replace,
    Clear,
}

/// Secret instruction embedded in account create/patch requests.
///
/// @spec docs/L1-api#secret-management
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretWriteRequest {
    #[serde(default)]
    pub mode: SecretWriteMode,
    pub password: Option<String>,
}

/// Request body for `POST /v1/accounts/{account_id}/oauth/start`.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartOAuthRequest {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
}

/// Request body for `POST /v1/oauth/start`.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartProviderOAuthRequest {
    pub provider: ProviderHint,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
}

/// Response body for `POST /v1/accounts/{account_id}/oauth/start`.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartOAuthResponse {
    pub authorization_url: String,
    pub state: String,
    pub redirect_uri: String,
}

/// Query parameters for the loopback OAuth redirect endpoint.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct OAuthCallbackQuery {
    pub state: String,
    pub code: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// Request body for `POST /v1/accounts`.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountRequest {
    pub id: Option<String>,
    pub name: String,
    pub full_name: Option<String>,
    #[serde(default)]
    pub email_patterns: Vec<String>,
    pub driver: Option<AccountDriver>,
    pub enabled: Option<bool>,
    pub appearance: Option<AccountAppearance>,
    #[serde(default)]
    pub transport: AccountTransportRequest,
    #[serde(default)]
    pub secret: SecretWriteRequest,
}

/// Request body for `PATCH /v1/accounts/{account_id}`. Omitted fields are preserved.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchAccountRequest {
    pub name: Option<String>,
    pub full_name: Option<String>,
    pub email_patterns: Option<Vec<String>>,
    pub driver: Option<AccountDriver>,
    pub enabled: Option<bool>,
    pub appearance: Option<AccountAppearance>,
    pub transport: Option<AccountTransportRequest>,
    pub secret: Option<SecretWriteRequest>,
}

impl From<AccountTransportRequest> for AccountTransportMutation {
    fn from(request: AccountTransportRequest) -> Self {
        Self {
            provider: request.provider,
            auth: request.auth,
            base_url: request.base_url,
            username: request.username,
            imap: request.imap,
            smtp: request.smtp,
        }
    }
}

impl From<SecretWriteMode> for RuntimeSecretWriteMode {
    fn from(mode: SecretWriteMode) -> Self {
        match mode {
            SecretWriteMode::Keep => Self::Keep,
            SecretWriteMode::Replace => Self::Replace,
            SecretWriteMode::Clear => Self::Clear,
        }
    }
}

impl From<SecretWriteRequest> for SecretWriteMutation {
    fn from(request: SecretWriteRequest) -> Self {
        Self {
            mode: request.mode.into(),
            password: request.password,
        }
    }
}

impl From<CreateAccountRequest> for CreateAccountMutation {
    fn from(request: CreateAccountRequest) -> Self {
        Self {
            id: request.id,
            name: request.name,
            full_name: request.full_name,
            email_patterns: request.email_patterns,
            driver: request.driver,
            enabled: request.enabled,
            appearance: request.appearance,
            transport: request.transport.into(),
            secret: request.secret.into(),
        }
    }
}

impl From<PatchAccountRequest> for PatchAccountMutation {
    fn from(request: PatchAccountRequest) -> Self {
        Self {
            name: request.name,
            full_name: request.full_name,
            email_patterns: request.email_patterns,
            driver: request.driver,
            enabled: request.enabled,
            appearance: request.appearance,
            transport: request.transport.map(Into::into),
            secret: request.secret.map(Into::into),
        }
    }
}
