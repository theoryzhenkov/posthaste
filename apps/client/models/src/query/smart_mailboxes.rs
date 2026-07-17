//! The smart-mailboxes family: every saved query with its rule and live
//! counts, in display order.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Read every smart mailbox. Carries no parameters; the enumeration is small
/// and bounded, answered whole.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
pub struct SmartMailboxesQuery {}

/// One smart mailbox: its saved configuration (rule included, so the editor
/// needs no second read) plus live unread/total counts.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SmartMailboxRow {
    #[ts(as = "crate::mirror::SmartMailboxId")]
    pub id: domain::SmartMailboxId,
    pub name: String,
    #[ts(as = "crate::mirror::SmartMailboxKind")]
    pub kind: domain::SmartMailboxKind,
    /// Identifies built-in smart mailboxes (e.g. `"inbox"`, `"trash"`).
    pub default_key: Option<String>,
    /// The mailbox role whose semantics apply to this view (e.g. `"trash"`),
    /// driving contextual actions like Delete Permanently.
    pub role: Option<String>,
    #[ts(optional = nullable, as = "Option<crate::mirror::SmartMailboxId>")]
    pub parent_id: Option<domain::SmartMailboxId>,
    /// The saved query rule this mailbox evaluates.
    #[ts(as = "crate::mirror::MailQueryRule")]
    pub rule: domain::MailQueryRule,
    #[ts(type = "number")]
    pub unread_messages: i64,
    #[ts(type = "number")]
    pub total_messages: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Every smart mailbox, in sidebar display order — the client renders the
/// rows verbatim.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SmartMailboxesResult {
    pub rows: Vec<SmartMailboxRow>,
}
