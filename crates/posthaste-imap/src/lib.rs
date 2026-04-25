//! IMAP/SMTP adapter boundary for traditional mail providers.
//!
//! This crate owns protocol-facing IMAP behavior while mapping server state
//! into Posthaste's domain model. The rest of the app should depend on domain
//! ports and records, not IMAP protocol types.
//!
//! @spec docs/L0-providers
//! @spec docs/L1-sync

mod discovery;
mod error;

pub use discovery::{
    discover_imap_account, imap_mailbox_id, map_imap_mailbox, normalize_imap_capabilities,
    DiscoveredImapAccount, DiscoveredImapMailbox, ImapConnectionConfig,
};
pub use error::ImapAdapterError;
