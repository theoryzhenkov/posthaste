use super::*;

/// Per-mailbox IMAP sync state.
///
/// @spec docs/L0-providers#imap-cursors-per-mailbox
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImapMailboxSyncState {
    pub mailbox_id: MailboxId,
    pub mailbox_name: String,
    pub uid_validity: ImapUidValidity,
    pub highest_uid: Option<ImapUid>,
    pub highest_modseq: Option<ImapModSeq>,
    pub updated_at: String,
}

/// IMAP location for a locally projected message.
///
/// Message identity and command addressability are separate for IMAP. Gmail can
/// use `X-GM-MSGID` as a stable message ID while still requiring a mailbox UID
/// for ordinary IMAP commands.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImapMessageLocation {
    pub message_id: MessageId,
    pub mailbox_id: MailboxId,
    pub uid_validity: ImapUidValidity,
    pub uid: ImapUid,
    pub modseq: Option<ImapModSeq>,
    pub updated_at: String,
}

/// Stable identity of one IMAP mailbox UID location.
///
/// This is the deletion address for mailbox-scoped IMAP observations. It omits
/// row metadata such as `modseq` and `updated_at` because deleting a location is
/// keyed only by the message identity and mailbox UID tuple.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImapMessageLocationKey {
    pub message_id: MessageId,
    pub mailbox_id: MailboxId,
    pub uid_validity: ImapUidValidity,
    pub uid: ImapUid,
}

impl ImapMessageLocation {
    pub fn key(&self) -> ImapMessageLocationKey {
        ImapMessageLocationKey {
            message_id: self.message_id.clone(),
            mailbox_id: self.mailbox_id.clone(),
            uid_validity: self.uid_validity,
            uid: self.uid,
        }
    }
}

/// Server state observed after selecting or examining an IMAP mailbox.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImapSelectedMailbox {
    pub mailbox_id: MailboxId,
    pub mailbox_name: String,
    pub uid_validity: ImapUidValidity,
    pub uid_next: Option<ImapUid>,
    pub highest_modseq: Option<ImapModSeq>,
}

/// Reason the IMAP driver must discard delta state and build an authoritative snapshot.
///
/// @spec docs/L0-providers#imap-delta-fallback
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapFullSyncReason {
    InitialSync,
    UidValidityChanged,
    MissingUidWatermark,
    FlagDeltaUnavailable,
    ProviderCanonicalizationRequired,
}

/// IMAP mailbox sync strategy selected from stored state and server capabilities.
///
/// @spec docs/L0-providers#imap-delta-fallback
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImapMailboxSyncPlan {
    FullSnapshot {
        reason: ImapFullSyncReason,
    },
    FetchNewByUid {
        after_uid: ImapUid,
    },
    CondstoreDelta {
        since_modseq: ImapModSeq,
        after_uid: Option<ImapUid>,
    },
    QresyncDelta {
        uid_validity: ImapUidValidity,
        since_modseq: ImapModSeq,
        after_uid: Option<ImapUid>,
    },
}

/// IMAP move implementation strategy selected from server capabilities.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapMoveStrategy {
    UidMoveWithCopyUid,
    UidMoveThenResync,
    CopyDeleteThenResync,
}

impl ImapMailboxSyncState {
    pub fn new(
        mailbox_id: MailboxId,
        mailbox_name: String,
        uid_validity: ImapUidValidity,
        updated_at: String,
    ) -> Self {
        Self {
            mailbox_id,
            mailbox_name,
            uid_validity,
            highest_uid: None,
            highest_modseq: None,
            updated_at,
        }
    }

    pub fn is_valid_for(&self, uid_validity: ImapUidValidity) -> bool {
        self.uid_validity == uid_validity
    }

    pub fn record_seen_uid(&mut self, uid: ImapUid) {
        self.highest_uid = Some(self.highest_uid.map_or(uid, |current| current.max(uid)));
    }

    pub fn record_highest_modseq(&mut self, modseq: ImapModSeq) {
        self.highest_modseq = Some(
            self.highest_modseq
                .map_or(modseq, |current| current.max(modseq)),
        );
    }
}
