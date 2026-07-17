//! The read side of the protocol: one typed [`Query`] enum posted to
//! `POST /query`, and the per-family result types its answers carry — one
//! submodule per family.
//!
//! Answers travel in [`QueryEnvelope`], `{ generation, data }`, where `data`
//! is the family's result verbatim (not variant-tagged): the caller knows
//! which family it asked, so the query it sent determines the decode type.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

mod accounts;
mod automation;
mod mail_list;
mod mailbox_counts;
mod message_detail;
mod pending_operations;
mod rev_log;
mod sender_addresses;
mod settings;
mod smart_mailboxes;
mod tags;
mod thread;

pub use accounts::*;
pub use automation::*;
pub use mail_list::*;
pub use mailbox_counts::*;
pub use message_detail::*;
pub use pending_operations::*;
pub use rev_log::*;
pub use sender_addresses::*;
pub use settings::*;
pub use smart_mailboxes::*;
pub use tags::*;
pub use thread::*;

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
    /// One message's full RFC 822 source — answered with
    /// [`MessageRawSourceResult`].
    MessageRawSource(MessageRawSourceQuery),
    /// Mailboxes with unread/total counts, derived live over the effective
    /// views — answered with [`MailboxCountsResult`].
    MailboxCounts(MailboxCountsQuery),
    /// Configured accounts with runtime health — answered with
    /// [`AccountsResult`].
    Accounts(AccountsQuery),
    /// One account's full configuration (transport included, secrets
    /// redacted) — answered with [`AccountSettingsResult`].
    AccountSettings(AccountSettingsQuery),
    /// Probe one account's provider connection — answered with
    /// [`VerifyAccountResult`].
    VerifyAccount(VerifyAccountQuery),
    /// An OAuth authorization descriptor for adding/re-crediting an account —
    /// answered with [`OauthStartResult`].
    OauthStart(OauthStartQuery),
    /// The outbox with verdicts — answered with [`PendingOperationsResult`].
    PendingOperations(PendingOperationsQuery),
    /// The global application settings document — answered with
    /// [`AppSettingsResult`].
    AppSettings(AppSettingsQuery),
    /// Every smart mailbox with its rule and live counts — answered with
    /// [`SmartMailboxesResult`].
    SmartMailboxes(SmartMailboxesQuery),
    /// Keyword-derived tags with counts — answered with [`TagsResult`].
    Tags(TagsQuery),
    /// Messages an automation condition would match today — answered with
    /// [`AutomationRulePreviewResult`].
    AutomationRulePreview(AutomationRulePreviewQuery),
    /// One account's undo/redo log and cursor — answered with
    /// [`RevLogResult`].
    RevLog(RevLogQuery),
    /// The compose autocomplete corpus — answered with
    /// [`SenderAddressesResult`].
    SenderAddresses(SenderAddressesQuery),
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
