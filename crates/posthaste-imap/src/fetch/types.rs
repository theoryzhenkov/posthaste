use super::*;

#[derive(Clone, Debug)]
pub struct ImapMailboxHeaderSnapshot {
    pub selected: ImapSelectedMailbox,
    pub headers: Vec<ImapMappedHeader>,
}

/// Header-level delta for mailboxes where UID is the only available sync state.
///
/// `current_uids` is an authoritative UID listing for deletion reconciliation,
/// while `headers` only contains records newer than the stored UID watermark.
#[derive(Clone, Debug)]
pub struct ImapMailboxUidDeltaSnapshot {
    pub selected: ImapSelectedMailbox,
    pub headers: Vec<ImapMappedHeader>,
    pub current_uids: Vec<ImapUid>,
}

/// Header-level records changed since a previously stored mailbox MODSEQ.
///
/// `headers` is intentionally partial: it contains only messages returned by
/// `UID FETCH ... (CHANGEDSINCE ...)`. Deletions are carried separately through
/// `vanished_uids` when the server supports QRESYNC.
#[derive(Clone, Debug)]
pub struct ImapChangedSinceSnapshot {
    pub selected: ImapSelectedMailbox,
    pub headers: Vec<ImapMappedHeader>,
    pub vanished_uids: Vec<ImapUid>,
    pub is_full_snapshot: bool,
}

/// Fetch and map header-level records for every message in one IMAP mailbox.
///
/// This performs a conservative full mailbox snapshot: `UID SEARCH ALL` obtains
/// candidate UIDs, then chunked `UID FETCH` retrieves only metadata and
/// RFC822 headers. Message bodies remain lazy.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
/// @spec docs/L1-sync#body-lazy

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImapFetchedHeaderWithMetadata {
    pub header: ImapFetchedHeader,
    pub gmail: ImapGmailMetadata,
}
