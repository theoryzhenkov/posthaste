//! The mailbox-counts family: mailboxes with unread/total counters, in
//! display order.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Read mailboxes and their unread/total counts, optionally for one account.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MailboxCountsQuery {
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::AccountId>")]
    pub account_id: Option<domain::AccountId>,
}

/// One mailbox with its counters, tagged with the account it belongs to.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MailboxCountsRow {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MailboxSummary")]
    pub mailbox: domain::MailboxSummary,
}

/// Every mailbox in scope with unread/total counts. Rows arrive in display
/// order: grouped by account (accounts in configuration order), role
/// mailboxes first in a fixed precedence, then named folders by name — the
/// sidebar renders them verbatim.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MailboxCountsResult {
    pub rows: Vec<MailboxCountsRow>,
}
