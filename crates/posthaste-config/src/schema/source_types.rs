use super::*;

// -- sources/<id>.toml --

/// TOML representation of an account source file (`sources/{id}.toml`).
///
/// @spec docs/L1-accounts#sourcesidtoml
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SourceToml {
    pub id: String,
    pub name: String,
    pub full_name: Option<String>,
    #[serde(default)]
    pub email_patterns: Vec<String>,
    pub driver: DriverToml,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub appearance: Option<AccountAppearanceToml>,
    #[serde(default)]
    pub transport: TransportToml,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Account driver variant: `jmap` or `mock`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverToml {
    Jmap,
    ImapSmtp,
    Mock,
}

/// TOML `[transport]` section: provider transport settings and credential
/// reference.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TransportToml {
    #[serde(default)]
    pub provider: ProviderHintToml,
    #[serde(default)]
    pub auth: ProviderAuthKindToml,
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub secret_ref: Option<SecretRefToml>,
    pub imap: Option<ImapTransportToml>,
    pub smtp: Option<SmtpTransportToml>,
}

/// TOML provider hint used for setup presets.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderHintToml {
    #[default]
    Generic,
    Gmail,
    Outlook,
    Icloud,
}

/// TOML provider authentication kind.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuthKindToml {
    #[default]
    Password,
    AppPassword,
    #[serde(rename = "oauth2")]
    OAuth2,
}

/// TOML TLS behavior for IMAP and SMTP endpoints.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportSecurityToml {
    #[default]
    Tls,
    StartTls,
    Plain,
}

/// TOML `[transport.imap]` section.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ImapTransportToml {
    pub host: String,
    pub port: u16,
    pub security: TransportSecurityToml,
}

/// TOML `[transport.smtp]` section.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SmtpTransportToml {
    pub host: String,
    pub port: u16,
    pub security: TransportSecurityToml,
}

/// TOML `[appearance]` section for user-customizable account marks.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "snake_case",
    tag = "kind"
)]
pub enum AccountAppearanceToml {
    Initials {
        initials: String,
        color_hue: u16,
    },
    Image {
        image_id: String,
        initials: String,
        color_hue: u16,
    },
}

/// TOML `[[automations]]` item for global automation rules.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AutomationRuleToml {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub triggers: Vec<AutomationTriggerToml>,
    #[serde(default)]
    pub backfill: bool,
    pub condition: RuleGroupToml,
    #[serde(default)]
    pub actions: Vec<AutomationActionToml>,
}

/// TOML automation trigger.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTriggerToml {
    MessageArrived,
    MessageChanged,
    Manual,
}

/// TOML automation action.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AutomationActionToml {
    ApplyTag { tag: String },
    RemoveTag { tag: String },
    MarkRead,
    MarkUnread,
    Flag,
    Unflag,
    MoveToMailbox { mailbox_id: String },
}

/// Credential reference: OS keyring (`os`) or environment variable (`env`).
///
/// @spec docs/L0-accounts#credential-storage
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SecretRefToml {
    pub kind: SecretKindToml,
    pub key: String,
}

/// Secret storage backend: environment variable or OS keyring.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKindToml {
    Env,
    Os,
}

/// Serde default for `SourceToml.enabled`.
fn default_true() -> bool {
    true
}
