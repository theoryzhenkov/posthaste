use std::{
    collections::{HashMap, HashSet},
    num::{NonZeroU32, NonZeroU64},
};

use imap_client::client::tokio::Client as ImapClient;
use imap_client::imap_types::body::{BodyStructure, SpecificFields};
use imap_client::imap_types::command::{CommandBody, FetchModifier};
use imap_client::imap_types::extensions::enable::CapabilityEnable;
use imap_client::imap_types::fetch::{
    MacroOrMessageDataItemNames, MessageDataItem, MessageDataItemName,
};
use imap_client::imap_types::flag::FlagFetch;
use imap_client::imap_types::response::{Data, StatusBody, StatusKind};
use imap_client::imap_types::search::SearchKey;
use imap_client::imap_types::sequence::SequenceSet;
use imap_client::tasks::tasks::TaskError;
use imap_client::tasks::Task;
use posthaste_domain::{
    GmailLabel, GmailMessageId, GmailThreadId, ImapGmailMetadata, ImapModSeq, ImapSelectedMailbox,
    ImapUid,
};
use posthaste_observability::{events, ph_debug, ph_info};

use crate::mailbox::examine_selected_mailbox;
use crate::message::imap_flags_include_deleted;
use crate::{
    imap_header_message_record_with_gmail_metadata, ImapAdapterError, ImapFetchedHeader,
    ImapMappedHeader,
};

const UID_FETCH_CHUNK_SIZE: usize = 128;

/// Header snapshot for one selected IMAP mailbox.
mod changed_since;
mod headers;
mod items;
mod types;

pub(crate) use changed_since::fetch_mailbox_changed_since_snapshot_with_client;
pub(crate) use headers::{
    fetch_mailbox_header_snapshot_with_client, fetch_mailbox_headers_after_uid_with_client,
    fetch_selected_mailbox_headers,
};
pub use items::{fetched_header_from_items, fetched_header_from_items_with_metadata};
pub use types::{
    ImapChangedSinceSnapshot, ImapFetchedHeaderWithMetadata, ImapMailboxHeaderSnapshot,
    ImapMailboxUidDeltaSnapshot,
};

#[cfg(test)]
use changed_since::ChangedSinceFetchTask;
#[cfg(test)]
use items::fetch_item_names;

#[cfg(test)]
mod tests;
