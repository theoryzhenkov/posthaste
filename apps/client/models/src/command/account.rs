//! Account intents: identity/appearance CRUD, transport endpoints, logo,
//! OAuth completion. Everything here is secrets-safe by construction — a
//! credential can only travel in the dedicated `setAccountSecret` command
//! (see [`crate::command::account_secret`]).

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::patch::FieldPatch;

/// Minimal account creation for [`crate::Command::CreateAccount`].
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountIntent {
    /// Display name for the account.
    pub name: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub full_name: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub signature: Option<String>,
    /// Email address patterns owned by this account.
    #[serde(default)]
    pub email_patterns: Vec<String>,
    /// Whether the account starts enabled; the backend default applies when
    /// absent.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub enabled: Option<bool>,
}

/// Identity/appearance patch for [`crate::Command::UpdateAccount`]; absent
/// fields are preserved. Enable/disable is the `enabled` field. The
/// clearable fields (`fullName`, `signature`) are [`FieldPatch`]es, so a
/// caller can also empty them explicitly.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccountIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub name: Option<String>,
    #[serde(default)]
    #[ts(optional, as = "Option<FieldPatch<String>>")]
    pub full_name: FieldPatch<String>,
    #[serde(default)]
    #[ts(optional, as = "Option<FieldPatch<String>>")]
    pub signature: FieldPatch<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub email_patterns: Option<Vec<String>>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub enabled: Option<bool>,
    /// Visual identity (initials/color, or a previously uploaded image).
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::AccountAppearance>")]
    pub appearance: Option<domain::AccountAppearance>,
}

/// Transport-endpoint patch for [`crate::Command::UpdateAccountTransport`];
/// absent fields are preserved. The clearable fields (`baseUrl`,
/// `username`) are [`FieldPatch`]es, so a caller can also empty them
/// explicitly. Deliberately has no field a credential could ride in: the
/// secret travels only in `setAccountSecret`.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccountTransportIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::ProviderHint>")]
    pub provider: Option<domain::ProviderHint>,
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::ProviderAuthKind>")]
    pub auth: Option<domain::ProviderAuthKind>,
    #[serde(default)]
    #[ts(optional, as = "Option<FieldPatch<String>>")]
    pub base_url: FieldPatch<String>,
    #[serde(default)]
    #[ts(optional, as = "Option<FieldPatch<String>>")]
    pub username: FieldPatch<String>,
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::ImapTransportSettings>")]
    pub imap: Option<domain::ImapTransportSettings>,
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::SmtpTransportSettings>")]
    pub smtp: Option<domain::SmtpTransportSettings>,
}

/// Target for [`crate::Command::DeleteAccount`]. Deletes the configuration,
/// the stored credential, and the account's local mail data.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAccountIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
}

/// Logo upload for [`crate::Command::SetAccountLogo`]. The image travels
/// base64 in the command body (the same convention as compose attachments);
/// the resulting image id lands in the account's appearance.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SetAccountLogoIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    pub mime_type: String,
    pub content_base64: String,
}

/// Callback descriptor for [`crate::Command::CompleteOauth`]: the provider
/// redirect parameters, handed back to finish an authorization the
/// `oauthStart` query began. The authorization code is single-use,
/// short-lived state-bound exchange input, not a stored credential; the
/// tokens the backend exchanges it for are stored through the secret store
/// and never surface on the wire.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CompleteOauthIntent {
    /// The CSRF state minted by `oauthStart`.
    pub state: String,
    /// The provider's authorization code from the redirect.
    pub code: String,
}
