//! Provider-mailbox intents: create, rename, delete, role assignment.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Content for [`crate::Command::CreateMailbox`]: a flat, top-level create —
/// a name only.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateMailboxIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    pub name: String,
}

/// Target + new name for [`crate::Command::RenameMailbox`].
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenameMailboxIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MailboxId")]
    pub mailbox_id: domain::MailboxId,
    pub name: String,
}

/// Target for [`crate::Command::DeleteMailbox`]. `removeEmails` is the
/// confirm-with-count safety flag: deleting a non-empty mailbox is refused
/// with a conflict unless it is true.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeleteMailboxIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MailboxId")]
    pub mailbox_id: domain::MailboxId,
    #[serde(default)]
    pub remove_emails: bool,
}

/// Target + role for [`crate::Command::SetMailboxRole`]. Assigning a role
/// held by another mailbox moves it; `null` clears the role.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SetMailboxRoleIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MailboxId")]
    pub mailbox_id: domain::MailboxId,
    /// One of the known roles (`inbox`, `archive`, `drafts`, `sent`, `junk`,
    /// `trash`, `snooze`), or `null` to clear.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub role: Option<String>,
}
