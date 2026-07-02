use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{MailboxId, MessageId, ProviderKind, ProviderProfile};

/// IMAP UID value. UIDs are scoped to one mailbox and one UIDVALIDITY value.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
mod capabilities;
mod mailbox_roles;
mod sync_state;
mod types;

pub use capabilities::{
    ImapCapabilities, ImapLabelSource, ImapMessageIdentitySource, ImapProviderFeatures,
    ImapThreadIdentitySource,
};
pub use mailbox_roles::imap_special_use_role;
pub use sync_state::{
    ImapFullSyncReason, ImapMailboxSyncPlan, ImapMailboxSyncState, ImapMessageLocation,
    ImapMessageLocationKey, ImapMoveStrategy, ImapSelectedMailbox,
};
pub use types::{
    GmailLabel, GmailMessageId, GmailThreadId, ImapGmailMetadata, ImapModSeq, ImapUid,
    ImapUidValidity,
};
