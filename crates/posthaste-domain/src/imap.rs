use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{MailboxId, MessageId};

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
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ImapUidValidity(pub u32);

/// IMAP CONDSTORE/QRESYNC modification sequence.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ImapModSeq(pub u64);

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
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImapFullSyncReason {
    InitialSync,
    UidValidityChanged,
    MissingUidWatermark,
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
/// and the local store has a previous MODSEQ. UID scanning is the baseline delta
/// path, but only inside the same UIDVALIDITY epoch.
///
/// @spec docs/L0-providers#imap-delta-fallback
pub fn plan_imap_mailbox_sync(
    capabilities: &ImapCapabilities,
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

    if capabilities.supports_qresync()
        && capabilities.supports_enable()
        && stored.highest_modseq.is_some()
        && selected.highest_modseq.is_some()
    {
        return ImapMailboxSyncPlan::QresyncDelta {
            uid_validity: selected.uid_validity,
            since_modseq: stored.highest_modseq.expect("checked above"),
            after_uid: stored.highest_uid,
        };
    }

    if capabilities.supports_condstore()
        && stored.highest_modseq.is_some()
        && selected.highest_modseq.is_some()
    {
        return ImapMailboxSyncPlan::CondstoreDelta {
            since_modseq: stored.highest_modseq.expect("checked above"),
            after_uid: stored.highest_uid,
        };
    }

    if let Some(after_uid) = stored.highest_uid {
        return ImapMailboxSyncPlan::FetchNewByUid { after_uid };
    }

    ImapMailboxSyncPlan::FullSnapshot {
        reason: ImapFullSyncReason::MissingUidWatermark,
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
mod tests {
    use super::*;

    #[test]
    fn imap_message_id_includes_uid_validity_to_avoid_uid_reuse_aliasing() {
        let mailbox_id = MailboxId::from("Inbox");

        let first = imap_message_id(&mailbox_id, ImapUidValidity(1), ImapUid(42));
        let after_reset = imap_message_id(&mailbox_id, ImapUidValidity(2), ImapUid(42));

        assert_ne!(first, after_reset);
    }

    #[test]
    fn imap_message_id_encodes_mailbox_id_without_delimiter_ambiguity() {
        let inbox = imap_message_id(&MailboxId::from("A:B"), ImapUidValidity(1), ImapUid(42));
        let other = imap_message_id(&MailboxId::from("A"), ImapUidValidity(1), ImapUid(42));

        assert_eq!(inbox.as_str(), "imap:1:42:413a42");
        assert_ne!(inbox, other);
    }

    #[test]
    fn mailbox_sync_state_tracks_high_watermarks_monotonically() {
        let mut state = ImapMailboxSyncState::new(
            MailboxId::from("Inbox"),
            "Inbox".to_string(),
            ImapUidValidity(7),
            "2026-04-25T00:00:00Z".to_string(),
        );

        state.record_seen_uid(ImapUid(20));
        state.record_seen_uid(ImapUid(10));
        state.record_highest_modseq(ImapModSeq(300));
        state.record_highest_modseq(ImapModSeq(200));

        assert_eq!(state.highest_uid, Some(ImapUid(20)));
        assert_eq!(state.highest_modseq, Some(ImapModSeq(300)));
        assert!(state.is_valid_for(ImapUidValidity(7)));
        assert!(!state.is_valid_for(ImapUidValidity(8)));
    }

    fn selected_mailbox(uid_validity: ImapUidValidity) -> ImapSelectedMailbox {
        ImapSelectedMailbox {
            mailbox_id: MailboxId::from("Inbox"),
            mailbox_name: "INBOX".to_string(),
            uid_validity,
            uid_next: Some(ImapUid(43)),
            highest_modseq: Some(ImapModSeq(400)),
        }
    }

    fn stored_state() -> ImapMailboxSyncState {
        let mut state = ImapMailboxSyncState::new(
            MailboxId::from("Inbox"),
            "INBOX".to_string(),
            ImapUidValidity(7),
            "2026-04-25T00:00:00Z".to_string(),
        );
        state.record_seen_uid(ImapUid(42));
        state.record_highest_modseq(ImapModSeq(300));
        state
    }

    #[test]
    fn planner_uses_qresync_when_server_and_state_support_it() {
        let capabilities = ImapCapabilities::from_tokens(["IMAP4rev1", "ENABLE", "QRESYNC"]);
        let stored = stored_state();

        let plan = plan_imap_mailbox_sync(
            &capabilities,
            Some(&stored),
            &selected_mailbox(ImapUidValidity(7)),
        );

        assert_eq!(
            plan,
            ImapMailboxSyncPlan::QresyncDelta {
                uid_validity: ImapUidValidity(7),
                since_modseq: ImapModSeq(300),
                after_uid: Some(ImapUid(42)),
            }
        );
    }

    #[test]
    fn planner_falls_back_to_full_snapshot_after_uidvalidity_change() {
        let capabilities = ImapCapabilities::from_tokens(["ENABLE", "QRESYNC"]);
        let stored = stored_state();

        let plan = plan_imap_mailbox_sync(
            &capabilities,
            Some(&stored),
            &selected_mailbox(ImapUidValidity(8)),
        );

        assert_eq!(
            plan,
            ImapMailboxSyncPlan::FullSnapshot {
                reason: ImapFullSyncReason::UidValidityChanged,
            }
        );
    }

    #[test]
    fn planner_uses_uid_scan_when_modseq_is_unavailable() {
        let capabilities = ImapCapabilities::from_tokens(["IMAP4rev1"]);
        let stored = stored_state();

        let plan = plan_imap_mailbox_sync(
            &capabilities,
            Some(&stored),
            &selected_mailbox(ImapUidValidity(7)),
        );

        assert_eq!(
            plan,
            ImapMailboxSyncPlan::FetchNewByUid {
                after_uid: ImapUid(42),
            }
        );
    }

    #[test]
    fn special_use_mapping_prefers_standard_attributes() {
        assert_eq!(
            imap_special_use_role("Sent Items", ["\\Sent"]),
            Some("sent")
        );
        assert_eq!(
            imap_special_use_role("INBOX", [] as [&str; 0]),
            Some("inbox")
        );
        assert_eq!(imap_special_use_role("Projects", ["\\HasNoChildren"]), None);
    }
}
