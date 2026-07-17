//! Smart-mailbox intents: create, update, delete, reset defaults.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Content for [`crate::Command::CreateSmartMailbox`]. The backend mints the
/// id; the created mailbox appears in the next `smartMailboxes` answer.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateSmartMailboxIntent {
    pub name: String,
    /// Optional view role (e.g. `"archive"`) giving the smart mailbox a
    /// built-in role's icon/accent and contextual actions.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub role: Option<String>,
    #[ts(as = "crate::mirror::MailQueryRule")]
    pub rule: domain::MailQueryRule,
}

/// Target + patch for [`crate::Command::UpdateSmartMailbox`]; absent fields
/// are preserved.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSmartMailboxIntent {
    #[ts(as = "crate::mirror::SmartMailboxId")]
    pub smart_mailbox_id: domain::SmartMailboxId,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub name: Option<String>,
    /// Set a role, or pass an empty string to clear it. Absent leaves the
    /// role unchanged.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub role: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::MailQueryRule>")]
    pub rule: Option<domain::MailQueryRule>,
}

/// Target for [`crate::Command::DeleteSmartMailbox`].
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSmartMailboxIntent {
    #[ts(as = "crate::mirror::SmartMailboxId")]
    pub smart_mailbox_id: domain::SmartMailboxId,
}

/// Content for [`crate::Command::ResetSmartMailboxes`]: restore the built-in
/// smart mailboxes to their default rules and ordering. User smart mailboxes
/// are untouched. Carries no parameters; it stays a struct so adding a scope
/// is not a wire break.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
pub struct ResetSmartMailboxesIntent {}
