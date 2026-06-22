use posthaste_domain::{MailboxId, MailboxRecord, MessageId, MessageRecord, SyncCursor};

#[cfg(test)]
use jmap_client::email;

mod cursor;
#[path = "sync/email.rs"]
mod email_sync;
mod mailbox;

#[cfg(test)]
pub(crate) use cursor::decode_email_cursor_state;
pub(crate) use cursor::encode_email_cursor_state;
#[cfg(test)]
use cursor::non_empty_state;
#[cfg(test)]
use email_sync::email_metadata_properties;
pub(crate) use email_sync::{fetch_email_sync, fetch_email_sync_streamed, StreamedEmailSync};
pub(crate) use mailbox::fetch_mailbox_sync;

/// Result of a mailbox sync cycle (delta or full).
///
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
pub(crate) struct MailboxSync {
    pub mailboxes: Vec<MailboxRecord>,
    pub deleted_mailbox_ids: Vec<MailboxId>,
    /// When `true`, the store treats this as an authoritative snapshot and
    /// prunes any local mailboxes missing from the result.
    pub replace_all_mailboxes: bool,
    pub cursor: SyncCursor,
}

/// Result of an email sync cycle (delta or full).
///
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
pub(crate) struct MessageSync {
    pub messages: Vec<MessageRecord>,
    pub deleted_message_ids: Vec<MessageId>,
    /// When `true`, the store treats this as an authoritative snapshot and
    /// prunes any local messages missing from the result.
    pub replace_all_messages: bool,
    pub cursor: SyncCursor,
}

#[cfg(test)]
mod tests;
