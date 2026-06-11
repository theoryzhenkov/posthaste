use posthaste_domain::{
    GmailMessageId, GmailThreadId, ImapCapabilities, ImapGmailMetadata, ImapMessageLocation,
    ImapModSeq, ImapSelectedMailbox, ImapUid, ImapUidValidity, MailboxId, MessageId,
};

use crate::{
    imap_header_message_record, imap_header_message_record_with_gmail_metadata, map_imap_mailbox,
    provider::ImapAdapterProviderProfile, ImapChangedSinceSnapshot, ImapFetchedHeader,
};

use super::*;

mod delta;
mod discovery;
mod gmail_canonical;
mod gmail_labels;
mod message_identity;
mod qresync;
