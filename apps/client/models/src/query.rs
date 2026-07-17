//! The read side of the protocol: one typed [`Query`] enum posted to
//! `POST /query`, and the per-family result types its answers carry.
//!
//! Answers travel in [`QueryEnvelope`], `{ generation, data }`, where `data`
//! is the family's result verbatim (not variant-tagged): the caller knows
//! which family it asked, so the query it sent determines the decode type.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One read request — every read family the API serves. Externally tagged, so
/// the wire shape is `{ "mailList": { ... } }`.
///
/// Free-text search is part of the mail list (its `freeText` field), not a
/// separate family: a search result is a mail list like any other, windowed
/// and sorted the same way.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Query {
    /// A windowed mail list — answered with [`MailListResult`].
    MailList(MailListQuery),
    /// All messages of one thread — answered with the thread view
    /// (`ThreadView`).
    Thread(ThreadQuery),
    /// One message with body content — answered with [`MessageDetailResult`].
    MessageDetail(MessageDetailQuery),
    /// Mailboxes with unread/total counts, derived live over the effective
    /// views — answered with [`MailboxCountsResult`].
    MailboxCounts(MailboxCountsQuery),
    /// Configured accounts with runtime health — answered with
    /// [`AccountsResult`].
    Accounts(AccountsQuery),
    /// The outbox with verdicts — answered with [`PendingOperationsResult`].
    PendingOperations(PendingOperationsQuery),
}

/// The answer envelope for every query: the family's result together with
/// the store generation it was computed at.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct QueryEnvelope<T> {
    /// The store generation observed before evaluation. An event-stream
    /// message at or past this value may supersede the answer.
    #[ts(type = "number")]
    pub generation: u64,
    /// The per-family result ([`MailListResult`], `ThreadView`, ...).
    pub data: T,
}

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
    /// Free-text search over subject, sender, recipients, preview, and the
    /// cached body index.
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

/// Read all messages of one provider thread.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ThreadQuery {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::ThreadId")]
    pub thread_id: domain::ThreadId,
}

/// Read one message with its body content.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetailQuery {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MessageId")]
    pub message_id: domain::MessageId,
}

/// One message opened for reading: its summary row plus sanitized bodies and
/// attachment metadata (attachment bytes are fetched as blobs).
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetailResult {
    #[ts(as = "crate::mirror::MessageSummary")]
    pub summary: domain::MessageSummary,
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    #[ts(as = "Vec<crate::mirror::MessageAttachment>")]
    pub attachments: Vec<domain::MessageAttachment>,
    /// Unsubscribe targets parsed from the message headers, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<crate::mirror::ListUnsubscribe>")]
    pub list_unsubscribe: Option<domain::ListUnsubscribe>,
}

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

/// Read the configured accounts with their runtime health. Carries no
/// parameters yet; it stays a struct so adding a filter is not a wire break.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
pub struct AccountsQuery {}

/// One account as the client renders it: identity plus live health. The full
/// settings tree (transport, secrets, appearance) is deliberately not on this
/// row — it belongs to the settings surface, not the mail surface.
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
}

/// Every configured account with health/status.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AccountsResult {
    pub rows: Vec<AccountRow>,
}

/// Read the outbox — pending intents and their verdicts — optionally for one
/// account.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingOperationsQuery {
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::AccountId>")]
    pub account_id: Option<domain::AccountId>,
}

/// One outbox operation as the client renders it: what it is, what it
/// targets, where it stands. Payload bodies stay in the backend.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingOperationRow {
    #[ts(as = "crate::mirror::OperationId")]
    pub id: domain::OperationId,
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::OperationKind")]
    pub kind: domain::OperationKind,
    #[ts(as = "crate::mirror::OperationState")]
    pub state: domain::OperationState,
    #[ts(as = "crate::mirror::OperationEntityKind")]
    pub entity_kind: domain::OperationEntityKind,
    /// The targeted message or draft id (possibly still a client-minted temp
    /// id awaiting provider reconciliation).
    pub entity_id: String,
    pub attempts: u32,
    pub last_error: Option<String>,
    /// Earliest submission time for a scheduled send (RFC 3339); absent for
    /// everything else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub send_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// The outbox in scope, newest first.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingOperationsResult {
    pub rows: Vec<PendingOperationRow>,
}
