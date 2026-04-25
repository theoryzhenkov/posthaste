//! IMAP/SMTP adapter boundary for traditional mail providers.
//!
//! This crate owns protocol-facing IMAP behavior while mapping server state
//! into Posthaste's domain model. The rest of the app should depend on domain
//! ports and records, not IMAP protocol types.
//!
//! @spec docs/L0-providers
//! @spec docs/L1-sync

mod body;
mod discovery;
mod error;
mod fetch;
mod gateway;
mod mailbox;
mod message;
mod sync;

pub use body::{fetch_message_body_by_location, fetched_body_from_items, imap_body_from_raw_mime};
pub use discovery::{
    discover_imap_account, imap_mailbox_id, map_imap_mailbox, normalize_imap_capabilities,
    DiscoveredImapAccount, DiscoveredImapMailbox, ImapConnectionConfig,
};
pub use error::ImapAdapterError;
pub use fetch::{
    fetch_mailbox_header_records, fetch_mailbox_header_snapshot, fetched_header_from_items,
    ImapMailboxHeaderSnapshot,
};
pub use gateway::LiveImapSmtpGateway;
pub use mailbox::{examine_imap_mailbox, selected_mailbox_from_examine};
pub use message::{
    imap_flag_keywords, imap_header_message_record, ImapFetchedHeader, ImapMappedHeader,
};
pub use sync::{
    imap_full_sync_batch, imap_mailbox_state_from_header_snapshot, imap_mailbox_sync_batch,
};
