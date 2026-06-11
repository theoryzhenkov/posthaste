use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{MailboxId, MessageId, ProviderKind, ProviderProfile};

/// IMAP UID value. UIDs are scoped to one mailbox and one UIDVALIDITY value.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ImapUid(pub u32);

/// IMAP UIDVALIDITY value. If this changes, cached UIDs for the mailbox are no
/// longer valid.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ImapUidValidity(pub u32);

/// IMAP CONDSTORE/QRESYNC modification sequence.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ImapModSeq(pub u64);

/// Gmail's stable IMAP message identifier from `X-GM-MSGID`.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GmailMessageId(pub u64);

/// Gmail's stable IMAP thread identifier from `X-GM-THRID`.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GmailThreadId(pub u64);

/// Gmail label name from `X-GM-LABELS`.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GmailLabel(pub String);

impl GmailLabel {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for GmailLabel {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for GmailLabel {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// Typed Gmail metadata carried by IMAP FETCH.
///
/// These values are present only when the protocol layer has requested and
/// parsed Gmail's `X-GM-*` FETCH extensions.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImapGmailMetadata {
    pub message_id: Option<GmailMessageId>,
    pub thread_id: Option<GmailThreadId>,
    #[serde(default)]
    pub labels_observed: bool,
    pub labels: Vec<GmailLabel>,
}

/// Normalized IMAP server capabilities used by the sync planner.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImapCapabilities {
    tokens: BTreeSet<String>,
}

impl ImapCapabilities {
    pub fn from_tokens(tokens: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let tokens = tokens
            .into_iter()
            .map(|token| token.as_ref().to_ascii_uppercase())
            .collect();
        Self { tokens }
    }

    pub fn contains(&self, token: &str) -> bool {
        self.tokens.contains(&token.to_ascii_uppercase())
    }

    pub fn supports_enable(&self) -> bool {
        self.contains("ENABLE")
    }

    pub fn supports_idle(&self) -> bool {
        self.contains("IDLE")
    }

    pub fn supports_special_use(&self) -> bool {
        self.contains("SPECIAL-USE") || self.contains("IMAP4REV2")
    }

    pub fn supports_uidplus(&self) -> bool {
        self.contains("UIDPLUS") || self.contains("IMAP4REV2")
    }

    pub fn supports_move(&self) -> bool {
        self.contains("MOVE") || self.contains("IMAP4REV2")
    }

    pub fn supports_condstore(&self) -> bool {
        self.contains("CONDSTORE") || self.supports_qresync()
    }

    pub fn supports_qresync(&self) -> bool {
        self.contains("QRESYNC")
    }

    pub fn supports_gmail_extensions(&self) -> bool {
        self.contains("X-GM-EXT-1")
    }
}

/// Remote identity source used to deduplicate IMAP messages.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapMessageIdentitySource {
    UidValidityUid,
    Rfc5322MessageId,
    GmailMessageId,
}

/// Remote thread source used when projecting conversations.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapThreadIdentitySource {
    Rfc5322Headers,
    GmailThreadId,
}

/// Source for mailbox/tag membership on IMAP accounts.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapLabelSource {
    MailboxMembership,
    GmailLabels,
}

/// Provider features inferred from IMAP capabilities.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImapProviderFeatures {
    pub message_identity: ImapMessageIdentitySource,
    pub thread_identity: ImapThreadIdentitySource,
    pub label_source: ImapLabelSource,
}

impl ImapProviderFeatures {
    pub fn from_capabilities(capabilities: &ImapCapabilities) -> Self {
        ProviderProfile::from_imap_capabilities(capabilities)
            .imap()
            .features()
    }

    pub fn for_provider_kind(kind: ProviderKind) -> Self {
        ProviderProfile::from_kind(kind).imap().features()
    }
}

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

/// Select the strongest correctness-preserving sync mode available for one mailbox.
///
/// QRESYNC and CONDSTORE are only usable when both the server advertises support
/// and the local store has a previous MODSEQ. Without MODSEQ, UID watermarks can
/// reconcile additions and expunges but cannot prove flag-only changes, so the
/// driver must refresh the mailbox metadata snapshot.
///
/// @spec docs/L0-providers#imap-delta-fallback
pub fn plan_imap_mailbox_sync(
    capabilities: &ImapCapabilities,
    provider: &ProviderProfile,
    stored: Option<&ImapMailboxSyncState>,
    selected: &ImapSelectedMailbox,
) -> ImapMailboxSyncPlan {
    let Some(stored) = stored else {
        return ImapMailboxSyncPlan::FullSnapshot {
            reason: ImapFullSyncReason::InitialSync,
        };
    };

    if !stored.is_valid_for(selected.uid_validity) {
        return ImapMailboxSyncPlan::FullSnapshot {
            reason: ImapFullSyncReason::UidValidityChanged,
        };
    }

    if let Some(reason) = provider.imap().required_full_sync_reason() {
        return ImapMailboxSyncPlan::FullSnapshot { reason };
    }

    if let (Some(since_modseq), Some(_)) = (stored.highest_modseq, selected.highest_modseq) {
        if capabilities.supports_qresync() && capabilities.supports_enable() {
            return ImapMailboxSyncPlan::QresyncDelta {
                uid_validity: selected.uid_validity,
                since_modseq,
                after_uid: stored.highest_uid,
            };
        }

        if capabilities.supports_condstore() {
            return ImapMailboxSyncPlan::CondstoreDelta {
                since_modseq,
                after_uid: stored.highest_uid,
            };
        }
    }

    if stored.highest_uid.is_some() {
        return ImapMailboxSyncPlan::FullSnapshot {
            reason: ImapFullSyncReason::FlagDeltaUnavailable,
        };
    }

    ImapMailboxSyncPlan::FullSnapshot {
        reason: ImapFullSyncReason::MissingUidWatermark,
    }
}

/// Select the safest available IMAP move strategy.
///
/// UIDPLUS lets the server report the destination UID after move/copy. Without
/// it, the command can still succeed, but local location state must be repaired
/// by a mailbox resync.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
pub fn plan_imap_move(capabilities: &ImapCapabilities) -> ImapMoveStrategy {
    if capabilities.supports_move() && capabilities.supports_uidplus() {
        ImapMoveStrategy::UidMoveWithCopyUid
    } else if capabilities.supports_move() {
        ImapMoveStrategy::UidMoveThenResync
    } else {
        ImapMoveStrategy::CopyDeleteThenResync
    }
}

/// Map SPECIAL-USE attributes into Posthaste's mailbox role vocabulary.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
pub fn imap_special_use_role(
    mailbox_name: &str,
    attributes: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<&'static str> {
    let normalized = attributes
        .into_iter()
        .map(|attribute| attribute.as_ref().to_ascii_uppercase())
        .collect::<BTreeSet<_>>();

    if normalized.contains("\\INBOX") || mailbox_name.eq_ignore_ascii_case("INBOX") {
        Some("inbox")
    } else if normalized.contains("\\SENT") {
        Some("sent")
    } else if normalized.contains("\\DRAFTS") {
        Some("drafts")
    } else if normalized.contains("\\TRASH") {
        Some("trash")
    } else if normalized.contains("\\JUNK") {
        Some("junk")
    } else if normalized.contains("\\ARCHIVE") {
        Some("archive")
    } else {
        None
    }
}

/// Build a stable local message ID for an IMAP message.
///
/// The mailbox identity and UIDVALIDITY are part of the ID so UID reuse after a
/// server-side mailbox reset cannot alias a previously cached message.
///
/// @spec docs/L0-providers#identity-and-threading
pub fn imap_message_id(
    mailbox_id: &MailboxId,
    uid_validity: ImapUidValidity,
    uid: ImapUid,
) -> MessageId {
    MessageId(format!(
        "imap:{}:{}:{}",
        uid_validity.0,
        uid.0,
        hex_encode(mailbox_id.as_str().as_bytes())
    ))
}

/// Build a stable local message ID from Gmail's `X-GM-MSGID`.
///
/// Gmail exposes the same message through multiple labels/mailboxes, so UID is
/// not the best deduplication key when the extension is available.
///
/// @spec docs/L0-providers#identity-and-threading
pub fn gmail_message_id(gmail_id: GmailMessageId) -> MessageId {
    MessageId(format!("imap:gmail:msgid:{}", gmail_id.0))
}

/// Build a stable local thread ID from Gmail's `X-GM-THRID`.
///
/// @spec docs/L0-providers#identity-and-threading
pub fn gmail_thread_id(gmail_id: GmailThreadId) -> crate::ThreadId {
    crate::ThreadId(format!("imap:gmail:thrid:{}", gmail_id.0))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[cfg(test)]
mod tests;
