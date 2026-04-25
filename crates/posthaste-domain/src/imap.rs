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
}
