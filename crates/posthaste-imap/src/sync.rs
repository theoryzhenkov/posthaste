use posthaste_domain::{
    AccountId, ImapMailboxSyncState, ImapMessageLocation, ImapUid, ImapUidValidity, MailboxId,
    MailboxRecord, MessageRecord, SyncBatch, SyncCursor, SyncObject,
};

use crate::{
    DiscoveredImapAccount, ImapChangedSinceSnapshot, ImapMailboxHeaderSnapshot, ImapMappedHeader,
};

/// Convert an IMAP mailbox discovery result into an authoritative mailbox
/// snapshot. Message sync is intentionally separate because it depends on
/// per-mailbox UIDVALIDITY and UID fetch state.
///
/// @spec docs/L0-providers#imap-discovery-runtime
pub fn imap_mailbox_sync_batch(
    _account_id: &AccountId,
    discovery: DiscoveredImapAccount,
    updated_at: String,
) -> SyncBatch {
    let mailboxes = discovery
        .mailboxes
        .iter()
        .filter(|mailbox| mailbox.selectable)
        .map(|mailbox| MailboxRecord {
            id: mailbox.id.clone(),
            name: mailbox.name.clone(),
            role: mailbox.role.map(str::to_string),
            unread_emails: 0,
            total_emails: 0,
        })
        .collect::<Vec<_>>();
    let cursor_state = mailbox_cursor_state(&mailboxes);

    SyncBatch {
        mailboxes,
        messages: Vec::new(),
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_mailbox_ids: Vec::new(),
        deleted_message_ids: Vec::new(),
        replace_all_mailboxes: true,
        replace_all_messages: false,
        cursors: vec![SyncCursor {
            object_type: SyncObject::Mailbox,
            state: cursor_state,
            updated_at,
        }],
    }
}

/// Convert IMAP discovery plus fetched mailbox headers into a full local
/// metadata snapshot.
///
/// The first IMAP sync path is intentionally full-snapshot based. UIDVALIDITY
/// and expunge handling make delta correctness mailbox-scoped; until that state
/// is wired through the runtime, the store's authoritative replacement contract
/// is the safer boundary.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
pub fn imap_full_sync_batch(
    account_id: &AccountId,
    discovery: DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
    mailbox_states: Vec<ImapMailboxSyncState>,
    updated_at: String,
) -> SyncBatch {
    let mut batch = imap_mailbox_sync_batch(account_id, discovery, updated_at.clone());
    let mut messages = Vec::with_capacity(headers.len());
    let mut locations = Vec::with_capacity(headers.len());

    for header in headers {
        messages.push(header.message);
        locations.push(header.location);
    }

    batch.imap_mailbox_states = mailbox_states;
    batch.messages = messages;
    batch.imap_message_locations = locations;
    batch.replace_all_messages = true;
    batch.cursors.push(SyncCursor {
        object_type: SyncObject::Message,
        state: message_cursor_state(&batch.messages, &batch.imap_message_locations),
        updated_at,
    });
    batch
}

pub fn imap_delta_sync_batch(
    account_id: &AccountId,
    discovery: DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
    mailbox_states: Vec<ImapMailboxSyncState>,
    local_locations: Vec<ImapMessageLocation>,
    updated_at: String,
) -> SyncBatch {
    let mut batch = imap_mailbox_sync_batch(account_id, discovery, updated_at.clone());
    let mut messages = Vec::with_capacity(headers.len());
    let mut locations = Vec::with_capacity(headers.len());
    let remote_locations = headers
        .iter()
        .map(|header| {
            (
                header.location.mailbox_id.clone(),
                header.location.uid_validity.0,
                header.location.uid,
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    let deleted_message_ids = local_locations
        .into_iter()
        .filter(|location| {
            !remote_locations.contains(&(
                location.mailbox_id.clone(),
                location.uid_validity.0,
                location.uid,
            ))
        })
        .map(|location| location.message_id)
        .collect::<Vec<_>>();

    for header in headers {
        messages.push(header.message);
        locations.push(header.location);
    }

    batch.imap_mailbox_states = mailbox_states;
    batch.messages = messages;
    batch.imap_message_locations = locations;
    batch.deleted_message_ids = deleted_message_ids;
    batch.replace_all_messages = false;
    batch.cursors.push(SyncCursor {
        object_type: SyncObject::Message,
        state: message_cursor_state(&batch.messages, &batch.imap_message_locations),
        updated_at,
    });
    batch
}

pub fn imap_condstore_delta_sync_batch(
    account_id: &AccountId,
    discovery: DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
    mailbox_states: Vec<ImapMailboxSyncState>,
    local_locations: Vec<ImapMessageLocation>,
    vanished_uids: Vec<(MailboxId, ImapUidValidity, ImapUid)>,
    updated_at: String,
) -> SyncBatch {
    let mut batch = imap_mailbox_sync_batch(account_id, discovery, updated_at.clone());
    let mut messages = Vec::with_capacity(headers.len());
    let mut locations = Vec::with_capacity(headers.len());
    let vanished_locations = vanished_uids
        .into_iter()
        .map(|(mailbox_id, uid_validity, uid)| (mailbox_id, uid_validity.0, uid))
        .collect::<std::collections::BTreeSet<_>>();
    let deleted_message_ids = local_locations
        .into_iter()
        .filter(|location| {
            vanished_locations.contains(&(
                location.mailbox_id.clone(),
                location.uid_validity.0,
                location.uid,
            ))
        })
        .map(|location| location.message_id)
        .collect::<Vec<_>>();

    for header in headers {
        messages.push(header.message);
        locations.push(header.location);
    }

    batch.imap_mailbox_states = mailbox_states;
    batch.messages = messages;
    batch.imap_message_locations = locations;
    batch.deleted_message_ids = deleted_message_ids;
    batch.replace_all_messages = false;
    batch.cursors.push(SyncCursor {
        object_type: SyncObject::Message,
        state: message_cursor_state(&batch.messages, &batch.imap_message_locations),
        updated_at,
    });
    batch
}

pub fn imap_mailbox_state_from_header_snapshot(
    snapshot: &ImapMailboxHeaderSnapshot,
    updated_at: String,
) -> ImapMailboxSyncState {
    ImapMailboxSyncState {
        mailbox_id: snapshot.selected.mailbox_id.clone(),
        mailbox_name: snapshot.selected.mailbox_name.clone(),
        uid_validity: snapshot.selected.uid_validity,
        highest_uid: snapshot
            .headers
            .iter()
            .map(|header| header.location.uid)
            .max(),
        highest_modseq: snapshot
            .headers
            .iter()
            .filter_map(|header| header.location.modseq)
            .max(),
        updated_at,
    }
}

pub fn imap_mailbox_state_from_changed_since_snapshot(
    stored: &ImapMailboxSyncState,
    snapshot: &ImapChangedSinceSnapshot,
    updated_at: String,
) -> ImapMailboxSyncState {
    let mut state = ImapMailboxSyncState {
        mailbox_id: snapshot.selected.mailbox_id.clone(),
        mailbox_name: snapshot.selected.mailbox_name.clone(),
        uid_validity: snapshot.selected.uid_validity,
        highest_uid: stored.highest_uid,
        highest_modseq: stored.highest_modseq,
        updated_at,
    };

    for header in &snapshot.headers {
        state.record_seen_uid(header.location.uid);
        if let Some(modseq) = header.location.modseq {
            state.record_highest_modseq(modseq);
        }
    }
    if let Some(highest_modseq) = snapshot.selected.highest_modseq {
        state.record_highest_modseq(highest_modseq);
    }

    state
}

fn mailbox_cursor_state(mailboxes: &[MailboxRecord]) -> String {
    let mut fingerprint = String::new();
    for mailbox in mailboxes {
        fingerprint.push_str(mailbox.id.as_str());
        fingerprint.push('\0');
        fingerprint.push_str(&mailbox.name);
        fingerprint.push('\0');
        fingerprint.push_str(mailbox.role.as_deref().unwrap_or(""));
        fingerprint.push('\0');
    }
    format!("imap-mailboxes:{}", hex::encode(fingerprint.as_bytes()))
}

fn message_cursor_state(messages: &[MessageRecord], locations: &[ImapMessageLocation]) -> String {
    let mut fingerprint = String::new();
    for message in messages {
        fingerprint.push_str(message.id.as_str());
        fingerprint.push('\0');
    }
    for location in locations {
        fingerprint.push_str(location.message_id.as_str());
        fingerprint.push('\0');
        fingerprint.push_str(location.mailbox_id.as_str());
        fingerprint.push('\0');
        fingerprint.push_str(&location.uid_validity.0.to_string());
        fingerprint.push('\0');
        fingerprint.push_str(&location.uid.0.to_string());
        fingerprint.push('\0');
    }
    format!("imap-messages:{}", hex::encode(fingerprint.as_bytes()))
}

#[cfg(test)]
mod tests {
    use posthaste_domain::{
        ImapCapabilities, ImapMessageLocation, ImapModSeq, ImapSelectedMailbox, ImapUid,
        ImapUidValidity, MailboxId, MessageId,
    };

    use crate::{
        imap_header_message_record, map_imap_mailbox, ImapChangedSinceSnapshot, ImapFetchedHeader,
    };

    use super::*;

    #[test]
    fn mailbox_discovery_becomes_authoritative_mailbox_snapshot() {
        let batch = imap_mailbox_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::default(),
                mailboxes: vec![
                    map_imap_mailbox("INBOX", ["\\Inbox"]),
                    map_imap_mailbox("[Gmail]", ["\\Noselect"]),
                    map_imap_mailbox("[Gmail]/Sent Mail", ["\\Sent"]),
                ],
            },
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert!(batch.replace_all_mailboxes);
        assert!(!batch.replace_all_messages);
        assert_eq!(batch.mailboxes.len(), 2);
        assert_eq!(
            batch.mailboxes[0].id,
            MailboxId::from("imap:mailbox:494e424f58")
        );
        assert_eq!(batch.mailboxes[0].role.as_deref(), Some("inbox"));
        assert_eq!(batch.mailboxes[1].role.as_deref(), Some("sent"));
        assert_eq!(batch.cursors[0].object_type, SyncObject::Mailbox);
        assert!(batch.cursors[0].state.starts_with("imap-mailboxes:"));
    }

    #[test]
    fn full_sync_batch_carries_messages_and_imap_locations() {
        let selected = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(9),
            uid_next: None,
            highest_modseq: None,
        };
        let mapped = imap_header_message_record(
            &selected,
            ImapFetchedHeader {
                mailbox_id: selected.mailbox_id.clone(),
                uid: ImapUid(42),
                modseq: Some(ImapModSeq(777)),
                flags: Vec::new(),
                rfc822_size: 512,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("mapped header");
        let expected_location = ImapMessageLocation {
            message_id: mapped.message.id.clone(),
            mailbox_id: selected.mailbox_id.clone(),
            uid_validity: ImapUidValidity(9),
            uid: ImapUid(42),
            modseq: Some(ImapModSeq(777)),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };

        let batch = imap_full_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::default(),
                mailboxes: vec![map_imap_mailbox("INBOX", ["\\Inbox"])],
            },
            vec![mapped],
            vec![ImapMailboxSyncState {
                mailbox_id: selected.mailbox_id.clone(),
                mailbox_name: "INBOX".to_string(),
                uid_validity: ImapUidValidity(9),
                highest_uid: Some(ImapUid(42)),
                highest_modseq: Some(ImapModSeq(777)),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            }],
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert!(batch.replace_all_messages);
        assert_eq!(batch.messages.len(), 1);
        assert_eq!(batch.imap_mailbox_states.len(), 1);
        assert_eq!(batch.imap_message_locations, vec![expected_location]);
        assert_eq!(batch.cursors[1].object_type, SyncObject::Message);
        assert!(batch.cursors[1].state.starts_with("imap-messages:"));
    }

    #[test]
    fn delta_sync_batch_deletes_local_locations_missing_from_remote_mailbox() {
        let selected = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(9),
            uid_next: None,
            highest_modseq: None,
        };
        let mapped = imap_header_message_record(
            &selected,
            ImapFetchedHeader {
                mailbox_id: selected.mailbox_id.clone(),
                uid: ImapUid(42),
                modseq: Some(ImapModSeq(777)),
                flags: Vec::new(),
                rfc822_size: 512,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("mapped header");
        let missing_location = ImapMessageLocation {
            message_id: MessageId::from("imap:9:41:696d61703a6d61696c626f783a34393465343234663538"),
            mailbox_id: selected.mailbox_id.clone(),
            uid_validity: selected.uid_validity,
            uid: ImapUid(41),
            modseq: Some(ImapModSeq(700)),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };

        let batch = imap_delta_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::default(),
                mailboxes: vec![map_imap_mailbox("INBOX", ["\\Inbox"])],
            },
            vec![mapped],
            vec![ImapMailboxSyncState {
                mailbox_id: selected.mailbox_id.clone(),
                mailbox_name: "INBOX".to_string(),
                uid_validity: selected.uid_validity,
                highest_uid: Some(ImapUid(42)),
                highest_modseq: Some(ImapModSeq(777)),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            }],
            vec![missing_location.clone()],
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert!(!batch.replace_all_messages);
        assert_eq!(batch.messages.len(), 1);
        assert_eq!(batch.deleted_message_ids, vec![missing_location.message_id]);
        assert_eq!(batch.cursors[1].object_type, SyncObject::Message);
    }

    #[test]
    fn condstore_delta_sync_batch_only_deletes_vanished_local_locations() {
        let selected = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(9),
            uid_next: None,
            highest_modseq: Some(ImapModSeq(900)),
        };
        let changed = imap_header_message_record(
            &selected,
            ImapFetchedHeader {
                mailbox_id: selected.mailbox_id.clone(),
                uid: ImapUid(42),
                modseq: Some(ImapModSeq(900)),
                flags: vec!["\\Seen".to_string()],
                rfc822_size: 512,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("mapped header");
        let unchanged_location = ImapMessageLocation {
            message_id: MessageId::from("imap:9:41:696d61703a6d61696c626f783a34393465343234663538"),
            mailbox_id: selected.mailbox_id.clone(),
            uid_validity: selected.uid_validity,
            uid: ImapUid(41),
            modseq: Some(ImapModSeq(700)),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };
        let vanished_location = ImapMessageLocation {
            message_id: MessageId::from("imap:9:40:696d61703a6d61696c626f783a34393465343234663538"),
            mailbox_id: selected.mailbox_id.clone(),
            uid_validity: selected.uid_validity,
            uid: ImapUid(40),
            modseq: Some(ImapModSeq(600)),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };

        let batch = imap_condstore_delta_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::default(),
                mailboxes: vec![map_imap_mailbox("INBOX", ["\\Inbox"])],
            },
            vec![changed],
            vec![ImapMailboxSyncState {
                mailbox_id: selected.mailbox_id.clone(),
                mailbox_name: "INBOX".to_string(),
                uid_validity: selected.uid_validity,
                highest_uid: Some(ImapUid(42)),
                highest_modseq: Some(ImapModSeq(900)),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            }],
            vec![unchanged_location.clone(), vanished_location.clone()],
            vec![(
                selected.mailbox_id.clone(),
                selected.uid_validity,
                vanished_location.uid,
            )],
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert!(!batch.replace_all_messages);
        assert_eq!(batch.messages.len(), 1);
        assert_eq!(
            batch.deleted_message_ids,
            vec![vanished_location.message_id]
        );
        assert!(!batch
            .deleted_message_ids
            .contains(&unchanged_location.message_id));
    }

    #[test]
    fn changed_since_snapshot_state_preserves_stored_uid_and_advances_modseq() {
        let selected = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(9),
            uid_next: None,
            highest_modseq: Some(ImapModSeq(900)),
        };
        let stored = ImapMailboxSyncState {
            mailbox_id: selected.mailbox_id.clone(),
            mailbox_name: "INBOX".to_string(),
            uid_validity: selected.uid_validity,
            highest_uid: Some(ImapUid(100)),
            highest_modseq: Some(ImapModSeq(700)),
            updated_at: "2026-04-24T00:00:00Z".to_string(),
        };
        let snapshot = ImapChangedSinceSnapshot {
            selected: selected.clone(),
            headers: Vec::new(),
            vanished_uids: Vec::new(),
            is_full_snapshot: false,
        };

        let state = imap_mailbox_state_from_changed_since_snapshot(
            &stored,
            &snapshot,
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert_eq!(state.highest_uid, Some(ImapUid(100)));
        assert_eq!(state.highest_modseq, Some(ImapModSeq(900)));
    }
}
