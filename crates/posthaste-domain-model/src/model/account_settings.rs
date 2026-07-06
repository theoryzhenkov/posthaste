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
    /// Per-mailbox sidebar color overrides. Each entry overrides the renderer's
    /// default (hash-derived) color for one provider mailbox. Pure presentation,
    /// keyed by `(source_id, mailbox_id)`; TOML source of truth.
    ///
    /// @spec docs/eph/RFC-L2-configuration-matrix
    #[serde(default)]
    pub mailbox_colors: Vec<MailboxColor>,
    /// Per-tag presentation overrides (color + icon), keyed by tag name. Tags
    /// themselves are keyword-derived and have no entity; this overlay gives a
    /// tag a foreground/background color and a lucide icon. Pure presentation,
    /// global by name; TOML source of truth. Absent fields fall back to the
    /// renderer's name-derived defaults.
    ///
    /// @spec docs/eph/DESIGN-L2-appearance-toml
    #[serde(default)]
    pub tags: Vec<TagAppearance>,
    /// User's explicit sidebar arrangement of smart mailboxes (by id). Acts as
    /// an override: ids listed here come first, in this order; any smart mailbox
    /// absent from the list falls back to the canonical default order (built-ins
    /// first) then creation order. Stale ids are ignored. Pure presentation.
    ///
    /// @spec docs/L1-accounts#sidebar-ordering
    #[serde(default)]
    pub smart_mailbox_order: Vec<SmartMailboxId>,
    /// User's explicit sidebar arrangement of accounts (by id). Same override
    /// semantics as [`smart_mailbox_order`](Self::smart_mailbox_order); accounts
    /// absent from the list fall back to name order.
    ///
    /// @spec docs/L1-accounts#sidebar-ordering
    #[serde(default)]
    pub account_order: Vec<AccountId>,
    /// Client-side, cross-device-synced sidebar "Groups" that visually cluster
    /// a source's mailboxes. Purely presentational — a Group never maps to a
    /// provider parent/child mailbox and no provider interaction occurs. Each
    /// group lists the member mailbox ids and a sidebar `order`. Deleting a
    /// group only drops the grouping; it never touches mailboxes or mail.
    ///
    /// @spec docs/eph/RFC-L2-mailbox-management#a4
    #[serde(default)]
    pub mailbox_groups: Vec<MailboxGroup>,
}

/// A client-side sidebar Group (presentation only): a named cluster of a
/// source's mailboxes, ordered in the sidebar by `order`. Groups never map to a
/// provider mailbox and nesting is out of scope (flat groups). A mailbox belongs
/// to at most one group (enforced in the assign UI).
///
/// @spec docs/eph/RFC-L2-mailbox-management#a4
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MailboxGroup {
    /// Stable client-generated id for the group.
    pub id: String,
    /// User-facing group name.
    pub name: String,
    /// Member mailbox ids (of this group's source). A mailbox appears in at most
    /// one group.
    #[serde(default)]
    pub mailbox_ids: Vec<String>,
    /// Sidebar sort position among a source's groups (ascending).
    pub order: i64,
}

/// A per-mailbox sidebar color override (presentation only). Overrides the
/// renderer's default hash-derived color for the mailbox identified by
/// `(source_id, mailbox_id)`.
///
/// @spec docs/eph/RFC-L2-configuration-matrix
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MailboxColor {
    pub source_id: AccountId,
    pub mailbox_id: MailboxId,
    /// Color hue (0–360).
    pub hue: u32,
}

/// A per-tag presentation override (presentation only), keyed by tag `name`.
/// Each field is optional and overrides the renderer's name-derived default for
/// that aspect; an absent field keeps the default.
///
/// @spec docs/eph/DESIGN-L2-appearance-toml
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TagAppearance {
    /// The tag (keyword) this override applies to.
    pub name: String,
    /// Foreground/text color (CSS color string, e.g. `#1f2937`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fg: Option<String>,
    /// Background color (CSS color string, e.g. `#dbeafe`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    /// Lucide icon name (e.g. `briefcase`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
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
