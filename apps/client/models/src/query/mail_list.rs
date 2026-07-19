//! The mail-list family: windowed, filtered, sorted message lists (free-text
//! search included).

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Sort order for a mail list. Defaults to date, newest first.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MailSort {
    /// The column to sort by.
    #[ts(as = "crate::mirror::MessageSortField")]
    pub field: domain::MessageSortField,
    /// True (the default) sorts descending — newest/highest first.
    pub descending: bool,
}

impl Default for MailSort {
    fn default() -> Self {
        Self {
            field: domain::MessageSortField::Date,
            descending: true,
        }
    }
}

/// Scope, filters, and window for a mail list read. Every field is optional:
/// the empty query is "all mail, date descending, first page".
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MailListQuery {
    /// Restrict to one account; absent spans all accounts.
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::AccountId>")]
    pub account_id: Option<domain::AccountId>,
    /// Restrict to one mailbox (requires `accountId` to be unambiguous for
    /// provider-scoped mailbox ids).
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::MailboxId>")]
    pub mailbox_id: Option<domain::MailboxId>,
    /// Restrict to one smart mailbox: the saved rule scopes the list, and the
    /// remaining filters AND on top of it. Mutually exclusive with
    /// `mailboxId`.
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::SmartMailboxId>")]
    pub smart_mailbox_id: Option<domain::SmartMailboxId>,
    /// Search text in the one query grammar: prefixed tokens
    /// (`conversation:`, `from:`, `is:`, ...) become field conditions; bare
    /// words search sender, subject, preview, and the cached body index. A
    /// string the grammar rejects fails the query as malformed.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub free_text: Option<String>,
    /// Keep only read (`true`) or unread (`false`) messages.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub is_read: Option<bool>,
    /// Keep only flagged (`true`) or unflagged (`false`) messages.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub is_flagged: Option<bool>,
    /// Keep only messages with (`true`) or without (`false`) attachments.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub has_attachment: Option<bool>,
    /// Sort order; absent means date descending.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub sort: Option<MailSort>,
    /// Maximum rows to return; the backend applies (and caps to) its own
    /// default when absent.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
    /// Opaque continuation returned by the previous page; absent starts from
    /// the top.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub cursor: Option<String>,
}

/// One page of a mail list: summary rows plus the continuation cursor.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MailListResult {
    /// The summary projection the list renders.
    #[ts(as = "Vec<crate::mirror::MessageSummary>")]
    pub rows: Vec<domain::MessageSummary>,
    /// Opaque cursor for the next page; absent means the list is exhausted.
    pub next_cursor: Option<String>,
}
