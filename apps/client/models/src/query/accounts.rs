//! The accounts family: the configured accounts with runtime health, one
//! account's full configuration (secrets redacted), the provider verification
//! probe, and the OAuth authorization descriptor.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Read the configured accounts with their runtime health. Carries no
/// parameters yet; it stays a struct so adding a filter is not a wire break.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
pub struct AccountsQuery {}

/// One account as the client renders it: identity plus live health. The full
/// settings tree (transport, secrets, appearance) is deliberately not on this
/// row — it belongs to the [`AccountSettingsQuery`] surface, not the mail
/// surface.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AccountRow {
    #[ts(as = "crate::mirror::AccountId")]
    pub id: domain::AccountId,
    pub name: String,
    pub full_name: Option<String>,
    pub enabled: bool,
    pub is_default: bool,
    /// Runtime health, owned by the account supervisor.
    #[ts(as = "crate::mirror::AccountStatus")]
    pub status: domain::AccountStatus,
    /// Push transport state.
    #[ts(as = "crate::mirror::PushStatus")]
    pub push: domain::PushStatus,
    pub last_sync_at: Option<String>,
    pub last_sync_error: Option<String>,
    /// Live detail for the sync cycle in flight, cleared when it comes to rest.
    /// Present only while `status` is `syncing`, so the UI can name the stage
    /// instead of showing a bare indeterminate spinner.
    #[serde(default)]
    #[ts(optional, as = "Option<crate::mirror::SyncProgress>")]
    pub sync_progress: Option<domain::SyncProgress>,
}

/// Every configured account with health/status.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AccountsResult {
    pub rows: Vec<AccountRow>,
}

/// Read one account's full configuration for the settings editor.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettingsQuery {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
}

/// An account's transport configuration as the client may see it. This view
/// is secrets-safe by construction: the stored credential surfaces only as a
/// redacted [`domain::SecretStatus`] (where it is stored, whether it is
/// configured, an optional label) — never the material, never the lookup key.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AccountTransportView {
    #[ts(as = "crate::mirror::ProviderHint")]
    pub provider: domain::ProviderHint,
    #[ts(as = "crate::mirror::ProviderAuthKind")]
    pub auth: domain::ProviderAuthKind,
    pub base_url: Option<String>,
    pub username: Option<String>,
    #[ts(as = "Option<crate::mirror::ImapTransportSettings>")]
    pub imap: Option<domain::ImapTransportSettings>,
    #[ts(as = "Option<crate::mirror::SmtpTransportSettings>")]
    pub smtp: Option<domain::SmtpTransportSettings>,
    /// Redacted credential state; the material itself never travels on the
    /// read side and is written only through the dedicated
    /// `setAccountSecret` command.
    #[ts(as = "crate::mirror::SecretStatus")]
    pub secret: domain::SecretStatus,
}

/// One account's full configuration: identity, driver, appearance, and the
/// secrets-safe transport view.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettingsResult {
    #[ts(as = "crate::mirror::AccountId")]
    pub id: domain::AccountId,
    pub name: String,
    pub full_name: Option<String>,
    pub signature: Option<String>,
    pub email_patterns: Vec<String>,
    #[ts(as = "crate::mirror::AccountDriver")]
    pub driver: domain::AccountDriver,
    pub enabled: bool,
    #[ts(optional = nullable, as = "Option<crate::mirror::AccountAppearance>")]
    pub appearance: Option<domain::AccountAppearance>,
    pub transport: AccountTransportView,
    pub created_at: String,
    pub updated_at: String,
}

/// Probe one account's provider connection with its stored transport and
/// credential. A read: nothing is stored, the provider is contacted once.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VerifyAccountQuery {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
}

/// The outcome of a provider verification probe.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VerifyAccountResult {
    /// True when the provider accepted the connection and credential.
    pub ok: bool,
    /// The identity email the provider reported, when it exposes one.
    pub identity_email: Option<String>,
    /// Whether the provider supports push for this account.
    pub push_supported: bool,
}

/// Read an OAuth authorization descriptor for a provider. The client id and
/// client secret here are the app's OAuth *registration* (bundled,
/// provider-published desktop-app config) — not account secret material,
/// which only ever travels in the dedicated `setAccountSecret` command.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OauthStartQuery {
    #[ts(as = "crate::mirror::ProviderHint")]
    pub provider: domain::ProviderHint,
    pub client_id: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub client_secret: Option<String>,
    pub redirect_uri: String,
}

/// The descriptor the client opens a browser with. The paired callback lands
/// as the `completeOauth` command carrying the returned `state`.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OauthStartResult {
    /// The provider authorization URL to open.
    pub authorization_url: String,
    /// Opaque CSRF state bound to this authorization attempt.
    pub state: String,
    /// The redirect URI the authorization was minted for.
    pub redirect_uri: String,
}
