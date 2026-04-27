//! IMAP/SMTP adapter boundary for traditional mail providers.
//!
//! This crate owns protocol-facing IMAP behavior while mapping server state
//! into Posthaste's domain model. The rest of the app should depend on domain
//! ports and records, not IMAP protocol types.
//!
//! @spec docs/L0-providers
//! @spec docs/L1-sync

mod body;
mod compose;
mod discovery;
mod error;
mod fetch;
mod gateway;
mod mailbox;
mod message;
mod mutation;
mod smtp;
mod sync;

pub use body::{
    fetch_message_body_by_location, fetch_raw_message_by_location, fetched_body_from_items,
    imap_attachment_bytes_from_raw_mime, imap_body_from_raw_mime, parse_imap_attachment_blob_id,
    raw_mime_from_items,
};
pub use compose::{fetch_imap_reply_context_by_location, imap_reply_context_from_raw_mime};
pub use discovery::{
    discover_imap_account, imap_mailbox_id, map_imap_mailbox, normalize_imap_capabilities,
    DiscoveredImapAccount, DiscoveredImapMailbox, ImapConnectionConfig,
};
pub use error::ImapAdapterError;
pub use fetch::{
    fetch_mailbox_changed_since_snapshot, fetch_mailbox_header_records,
    fetch_mailbox_header_snapshot, fetch_mailbox_headers_after_uid, fetched_header_from_items,
    ImapChangedSinceSnapshot, ImapMailboxHeaderSnapshot, ImapMailboxUidDeltaSnapshot,
};
pub use gateway::LiveImapSmtpGateway;
pub use mailbox::{examine_imap_mailbox, selected_mailbox_from_examine};
pub use message::{
    imap_flag_keywords, imap_header_message_record, ImapFetchedHeader, ImapMappedHeader,
};
pub use mutation::{
    apply_imap_keyword_delta_by_location, copy_imap_message_to_mailbox_by_location,
    expunge_imap_message_by_location, imap_flags_for_keywords, imap_mailbox_replacement_delta,
    mark_imap_message_deleted_by_location, move_imap_message_to_mailbox_by_location,
    ImapMailboxReplacementDelta,
};
pub use smtp::{
    append_smtp_sent_copy, build_smtp_message, render_smtp_markdown, send_smtp_message,
    smtp_mailbox_for_recipient, smtp_sent_copy_strategy, submit_smtp_message, SmtpConnectionConfig,
    SmtpSentCopyStrategy, SubmittedSmtpMessage,
};
pub use sync::{
    imap_condstore_delta_sync_batch, imap_delta_sync_batch, imap_full_sync_batch,
    imap_mailbox_state_from_changed_since_snapshot, imap_mailbox_state_from_header_snapshot,
    imap_mailbox_sync_batch,
};
