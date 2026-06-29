use super::*;

/// Global application settings shared across all accounts.
///
/// @spec docs/L1-accounts#toml-schema
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AppSettings {
    pub default_account_id: Option<AccountId>,
    #[serde(default)]
    pub cache_policy: CachePolicy,
    #[serde(default)]
    pub automation_rules: Vec<AutomationRule>,
    #[serde(default)]
    pub automation_drafts: Vec<AutomationRule>,
    /// UI appearance/theme prefs (TOML source of truth; renderer mirrors for boot).
    ///
    /// @spec docs/eph/DESIGN-L2-appearance-toml
    #[serde(default)]
    pub appearance: Option<Appearance>,
    /// Notification policy (TOML source of truth; OS delivery permission stays local).
    ///
    /// @spec docs/eph/RFC-L2-configuration-matrix
    #[serde(default)]
    pub notifications: Option<Notifications>,
}

/// Backend driver type for an account.
///
/// @spec docs/L1-accounts#toml-schema
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AccountDriver {
    Jmap,
    ImapSmtp,
    Mock,
}

/// Provider-level capabilities that affect domain planning.
///
/// @spec docs/L1-sync#local-cache-planning
/// @spec docs/L1-jmap#push
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountDriverCapabilities {
    pub cache_fetch_unit: CacheFetchUnit,
    pub supports_push: bool,
}

impl AccountDriver {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Jmap => "jmap",
            Self::ImapSmtp => "imap_smtp",
            Self::Mock => "mock",
        }
    }

    pub fn capabilities(&self) -> AccountDriverCapabilities {
        match self {
            Self::Jmap => AccountDriverCapabilities {
                cache_fetch_unit: CacheFetchUnit::BodyOnly,
                supports_push: true,
            },
            Self::ImapSmtp => AccountDriverCapabilities {
                cache_fetch_unit: CacheFetchUnit::RawMessage,
                supports_push: false,
            },
            Self::Mock => AccountDriverCapabilities {
                cache_fetch_unit: CacheFetchUnit::BodyOnly,
                supports_push: false,
            },
        }
    }
}

/// Storage backend for account credentials.
///
/// @spec docs/L1-api#secret-management
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SecretKind {
    /// Credential read from an environment variable.
    Env,
    /// Credential stored in the OS keyring (macOS Keychain).
    Os,
}

/// Pointer to a stored secret, combining storage kind and lookup key.
///
/// @spec docs/L1-api#secret-management
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SecretRef {
    pub kind: SecretKind,
    pub key: String,
}

/// Alias for [`SecretKind`], used in API responses to describe where a secret is stored.
///
/// @spec docs/L1-api#secret-management
pub type SecretStorage = SecretKind;

/// Redacted secret status returned in API responses. Never contains the secret value.
///
/// @spec docs/L1-api#secret-management
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SecretStatus {
    pub storage: SecretStorage,
    pub configured: bool,
    pub label: Option<String>,
}

/// User-selected provider hint for traditional mail account setup.
///
/// @spec docs/L0-providers#driver-model
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ProviderHint {
    #[default]
    Generic,
    Gmail,
    Outlook,
    Icloud,
}

impl From<ProviderKind> for ProviderHint {
    fn from(provider: ProviderKind) -> Self {
        match provider {
            ProviderKind::Generic => Self::Generic,
            ProviderKind::Gmail => Self::Gmail,
            ProviderKind::Outlook => Self::Outlook,
            ProviderKind::Icloud => Self::Icloud,
        }
    }
}

/// Authentication mode used by the selected provider transport.
///
/// @spec docs/L0-providers#authentication
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ProviderAuthKind {
    #[default]
    Password,
    AppPassword,
    #[serde(rename = "oauth2")]
    OAuth2,
}

/// TLS behavior for IMAP and SMTP endpoints.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum TransportSecurity {
    #[default]
    Tls,
    StartTls,
    Plain,
}

/// IMAP endpoint settings for traditional provider sync.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ImapTransportSettings {
    pub host: String,
    pub port: u16,
    pub security: TransportSecurity,
}

/// SMTP endpoint settings for traditional provider submission.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SmtpTransportSettings {
    pub host: String,
    pub port: u16,
    pub security: TransportSecurity,
}

/// Transport-layer settings for connecting to a provider.
///
/// @spec docs/L1-accounts#toml-schema
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountTransportSettings {
    #[serde(default)]
    pub provider: ProviderHint,
    #[serde(default)]
    pub auth: ProviderAuthKind,
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub secret_ref: Option<SecretRef>,
    pub imap: Option<ImapTransportSettings>,
    pub smtp: Option<SmtpTransportSettings>,
}

impl AccountTransportSettings {
    pub fn provider_kind(&self) -> ProviderKind {
        ProviderKind::from(&self.provider)
    }
}

/// User-facing visual identity for an account.
///
/// @spec docs/L1-accounts#toml-schema
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AccountAppearance {
    Initials {
        initials: String,
        #[cfg_attr(feature = "openapi", schema(rename = "colorHue"))]
        color_hue: u16,
    },
    Image {
        #[cfg_attr(feature = "openapi", schema(rename = "imageId"))]
        image_id: String,
        initials: String,
        #[cfg_attr(feature = "openapi", schema(rename = "colorHue"))]
        color_hue: u16,
    },
}
