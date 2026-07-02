use posthaste_domain_model::{
    GmailMessageId, GmailThreadId, ImapCapabilities, ImapFullSyncReason, ImapMailboxSyncPlan,
    ImapMailboxSyncState, ImapMoveStrategy, ImapSelectedMailbox, ImapUid, ImapUidValidity,
    MailboxId, MessageId, ProviderProfile,
};
#[cfg(test)]
use posthaste_domain_model::{
    imap_special_use_role, ImapLabelSource, ImapMessageIdentitySource, ImapModSeq,
    ImapProviderFeatures, ImapThreadIdentitySource, ProviderKind,
};

mod identities;
mod planning;

pub use identities::{gmail_message_id, gmail_thread_id, imap_message_id};
pub use planning::{plan_imap_mailbox_sync, plan_imap_move};

#[cfg(test)]
mod tests;
