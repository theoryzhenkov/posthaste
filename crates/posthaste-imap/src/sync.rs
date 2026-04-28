use std::collections::{BTreeMap, BTreeSet};

use posthaste_domain::{
    AccountId, ImapMailboxSyncState, ImapMessageLocation, ImapMessageLocationKey, ImapUid,
    ImapUidValidity, MailboxId, MailboxRecord, MessageId, MessageRecord, SyncBatch, SyncCursor,
    SyncObject,
};

use crate::{
    provider::ImapAdapterProviderProfile, DiscoveredImapAccount, ImapChangedSinceSnapshot,
    ImapMailboxHeaderSnapshot, ImapMappedHeader,
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
        deleted_imap_message_locations: Vec::new(),
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
    let (messages, locations) = messages_and_locations_for_batch(&discovery, headers);
    let mut batch = imap_mailbox_sync_batch(account_id, discovery, updated_at.clone());

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
    let headers = project_imap_headers(&discovery, headers);
    let remote_locations = headers
        .iter()
        .map(|header| header.location.key())
        .collect::<BTreeSet<_>>();
    let (mut messages, locations) = messages_and_locations_from_headers(headers);
    let mut batch = imap_mailbox_sync_batch(account_id, discovery, updated_at.clone());
    let deleted_imap_message_locations =
        deleted_locations_missing_from_remote(&local_locations, &remote_locations);
    let deleted_message_ids = deleted_message_ids_for_deleted_locations(
        &local_locations,
        &deleted_imap_message_locations,
        &locations,
    );
    preserve_delta_mailboxes_from_locations(
        &mut messages,
        &local_locations,
        &deleted_imap_message_locations,
        &locations,
    );

    batch.imap_mailbox_states = mailbox_states;
    batch.messages = messages;
    batch.imap_message_locations = locations;
    batch.deleted_imap_message_locations = deleted_imap_message_locations;
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
    let (mut messages, locations) = messages_and_locations_for_batch(&discovery, headers);
    let mut batch = imap_mailbox_sync_batch(account_id, discovery, updated_at.clone());
    let vanished_locations = vanished_uids.into_iter().collect::<BTreeSet<_>>();
    let deleted_imap_message_locations =
        deleted_locations_matching_vanished_uids(&local_locations, &vanished_locations);
    let deleted_message_ids = deleted_message_ids_for_deleted_locations(
        &local_locations,
        &deleted_imap_message_locations,
        &locations,
    );
    preserve_delta_mailboxes_from_locations(
        &mut messages,
        &local_locations,
        &deleted_imap_message_locations,
        &locations,
    );

    batch.imap_mailbox_states = mailbox_states;
    batch.messages = messages;
    batch.imap_message_locations = locations;
    batch.deleted_imap_message_locations = deleted_imap_message_locations;
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

fn messages_and_locations_for_batch(
    discovery: &DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
) -> (Vec<MessageRecord>, Vec<ImapMessageLocation>) {
    let headers = project_imap_headers(discovery, headers);
    messages_and_locations_from_headers(headers)
}

fn messages_and_locations_from_headers(
    headers: Vec<ImapMappedHeader>,
) -> (Vec<MessageRecord>, Vec<ImapMessageLocation>) {
    let mut messages_by_id = BTreeMap::<MessageId, MessageRecord>::new();
    let mut locations = Vec::with_capacity(headers.len());

    for header in headers {
        messages_by_id
            .entry(header.message.id.clone())
            .or_insert(header.message);
        locations.push(header.location);
    }

    (messages_by_id.into_values().collect(), locations)
}

fn project_imap_headers(
    discovery: &DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
) -> Vec<ImapMappedHeader> {
    ImapAdapterProviderProfile::from_discovery(discovery).project_headers(headers)
}

fn deleted_locations_missing_from_remote(
    local_locations: &[ImapMessageLocation],
    remote_locations: &BTreeSet<ImapMessageLocationKey>,
) -> Vec<ImapMessageLocationKey> {
    local_locations
        .iter()
        .map(ImapMessageLocation::key)
        .filter(|key| !remote_locations.contains(key))
        .collect()
}

fn deleted_locations_matching_vanished_uids(
    local_locations: &[ImapMessageLocation],
    vanished_locations: &BTreeSet<(MailboxId, ImapUidValidity, ImapUid)>,
) -> Vec<ImapMessageLocationKey> {
    local_locations
        .iter()
        .map(ImapMessageLocation::key)
        .filter(|key| {
            vanished_locations.contains(&(key.mailbox_id.clone(), key.uid_validity, key.uid))
        })
        .collect()
}

fn deleted_message_ids_for_deleted_locations(
    local_locations: &[ImapMessageLocation],
    deleted_locations: &[ImapMessageLocationKey],
    new_locations: &[ImapMessageLocation],
) -> Vec<MessageId> {
    let deleted_keys = deleted_locations.iter().cloned().collect::<BTreeSet<_>>();
    let mut remaining_location_counts = BTreeMap::<MessageId, usize>::new();

    for location in local_locations {
        if deleted_keys.contains(&location.key()) {
            continue;
        }
        *remaining_location_counts
            .entry(location.message_id.clone())
            .or_default() += 1;
    }
    for location in new_locations {
        *remaining_location_counts
            .entry(location.message_id.clone())
            .or_default() += 1;
    }

    deduplicate_message_ids(
        deleted_locations
            .iter()
            .filter(|key| {
                remaining_location_counts
                    .get(&key.message_id)
                    .copied()
                    .unwrap_or(0)
                    == 0
            })
            .map(|key| key.message_id.clone())
            .collect(),
    )
}

fn preserve_delta_mailboxes_from_locations(
    messages: &mut [MessageRecord],
    local_locations: &[ImapMessageLocation],
    deleted_locations: &[ImapMessageLocationKey],
    new_locations: &[ImapMessageLocation],
) {
    let deleted_keys = deleted_locations.iter().cloned().collect::<BTreeSet<_>>();
    let mut mailbox_ids_by_message = BTreeMap::<MessageId, BTreeSet<MailboxId>>::new();

    for location in local_locations {
        if deleted_keys.contains(&location.key()) {
            continue;
        }
        mailbox_ids_by_message
            .entry(location.message_id.clone())
            .or_default()
            .insert(location.mailbox_id.clone());
    }
    for location in new_locations {
        mailbox_ids_by_message
            .entry(location.message_id.clone())
            .or_default()
            .insert(location.mailbox_id.clone());
    }

    for message in messages {
        if let Some(mailbox_ids) = mailbox_ids_by_message.remove(&message.id) {
            message.mailbox_ids = mailbox_ids.into_iter().collect();
        }
    }
}

fn deduplicate_message_ids(mut message_ids: Vec<MessageId>) -> Vec<MessageId> {
    message_ids.sort();
    message_ids.dedup();
    message_ids
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
        imap_header_message_record, map_imap_mailbox, provider::ImapAdapterProviderProfile,
        ImapChangedSinceSnapshot, ImapFetchedHeader,
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
                has_attachment: false,
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
    fn gmail_sync_canonicalizes_one_message_observed_in_multiple_mailboxes() {
        let sent = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:sent"),
            mailbox_name: "[Gmail]/Sent Mail".to_string(),
            uid_validity: ImapUidValidity(7),
            uid_next: None,
            highest_modseq: None,
        };
        let starred = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:starred"),
            mailbox_name: "[Gmail]/Starred".to_string(),
            uid_validity: ImapUidValidity(8),
            uid_next: None,
            highest_modseq: None,
        };
        let all_mail = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:all"),
            mailbox_name: "[Gmail]/All Mail".to_string(),
            uid_validity: ImapUidValidity(9),
            uid_next: None,
            highest_modseq: None,
        };
        let sent_header = imap_header_message_record(
            &sent,
            ImapFetchedHeader {
                mailbox_id: sent.mailbox_id.clone(),
                uid: ImapUid(12),
                modseq: None,
                flags: vec!["\\Seen".to_string()],
                rfc822_size: 512,
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\nMessage-ID: <gmail-1@example.test>\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("sent header");
        let starred_header = imap_header_message_record(
            &starred,
            ImapFetchedHeader {
                mailbox_id: starred.mailbox_id.clone(),
                uid: ImapUid(44),
                modseq: None,
                flags: vec!["\\Flagged".to_string()],
                rfc822_size: 512,
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\nMessage-ID: <gmail-1@example.test>\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("starred header");
        let all_mail_header = imap_header_message_record(
            &all_mail,
            ImapFetchedHeader {
                mailbox_id: all_mail.mailbox_id.clone(),
                uid: ImapUid(88),
                modseq: None,
                flags: vec!["\\Seen".to_string()],
                rfc822_size: 512,
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\nMessage-ID: <gmail-1@example.test>\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("all mail header");

        let batch = imap_full_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::from_tokens(["IMAP4rev1", "X-GM-EXT-1"]),
                mailboxes: vec![
                    map_imap_mailbox("[Gmail]/Sent Mail", ["\\Sent"]),
                    map_imap_mailbox("[Gmail]/Starred", ["\\Flagged"]),
                    map_imap_mailbox("[Gmail]/All Mail", ["\\All"]),
                ],
            },
            vec![sent_header, starred_header, all_mail_header],
            Vec::new(),
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert_eq!(batch.messages.len(), 1);
        assert_eq!(
            batch.messages[0].id.as_str(),
            "imap:gmail:rfc822msgid:676d61696c2d31406578616d706c652e74657374"
        );
        assert_eq!(
            batch.messages[0].mailbox_ids,
            vec![
                MailboxId::from("imap:mailbox:all"),
                MailboxId::from("imap:mailbox:sent"),
                MailboxId::from("imap:mailbox:starred")
            ]
        );
        assert_eq!(batch.messages[0].keywords, vec!["$flagged", "$seen"]);
        assert_eq!(batch.imap_message_locations.len(), 3);
        assert!(batch
            .imap_message_locations
            .iter()
            .all(|location| location.message_id == batch.messages[0].id));
    }

    #[test]
    fn generic_imap_keeps_mailbox_uid_identity_even_with_shared_rfc_message_id() {
        let inbox = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:inbox"),
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(7),
            uid_next: None,
            highest_modseq: None,
        };
        let archive = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:archive"),
            mailbox_name: "Archive".to_string(),
            uid_validity: ImapUidValidity(8),
            uid_next: None,
            highest_modseq: None,
        };
        let inbox_header = imap_header_message_record(
            &inbox,
            ImapFetchedHeader {
                mailbox_id: inbox.mailbox_id.clone(),
                uid: ImapUid(12),
                modseq: None,
                flags: Vec::new(),
                rfc822_size: 512,
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\nMessage-ID: <copied@example.test>\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("inbox header");
        let archive_header = imap_header_message_record(
            &archive,
            ImapFetchedHeader {
                mailbox_id: archive.mailbox_id.clone(),
                uid: ImapUid(44),
                modseq: None,
                flags: Vec::new(),
                rfc822_size: 512,
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\nMessage-ID: <copied@example.test>\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("archive header");

        let batch = imap_full_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::from_tokens(["IMAP4rev1"]),
                mailboxes: vec![
                    map_imap_mailbox("INBOX", ["\\Inbox"]),
                    map_imap_mailbox("Archive", ["\\Archive"]),
                ],
            },
            vec![inbox_header, archive_header],
            Vec::new(),
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert_eq!(batch.messages.len(), 2);
        assert_ne!(batch.messages[0].id, batch.messages[1].id);
        assert_eq!(batch.imap_message_locations.len(), 2);
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
                has_attachment: false,
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
        assert_eq!(
            batch.deleted_imap_message_locations,
            vec![missing_location.key()]
        );
        assert_eq!(batch.deleted_message_ids, vec![missing_location.message_id]);
        assert_eq!(batch.cursors[1].object_type, SyncObject::Message);
    }

    // spec: docs/L0-testing#provider-observation-contracts
    #[test]
    fn gmail_delta_deletion_detection_uses_canonical_remote_location_keys() {
        let selected = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:inbox"),
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
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\nMessage-ID: <gmail-delta@example.test>\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("mapped header");
        let local_location = ImapMessageLocation {
            message_id: ImapAdapterProviderProfile::gmail().canonical_message_id(&mapped.message),
            ..mapped.location.clone()
        };

        let batch = imap_delta_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::from_tokens(["IMAP4rev1", "X-GM-EXT-1"]),
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
            vec![local_location],
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert!(batch.deleted_imap_message_locations.is_empty());
        assert!(batch.deleted_message_ids.is_empty());
    }

    // spec: docs/L0-testing#provider-observation-contracts
    #[test]
    fn gmail_partial_flag_delta_preserves_unobserved_existing_mailboxes() {
        let all_mail = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:all"),
            mailbox_name: "[Gmail]/All Mail".to_string(),
            uid_validity: ImapUidValidity(8),
            uid_next: None,
            highest_modseq: Some(ImapModSeq(902)),
        };
        let all_mail_header = imap_header_message_record(
            &all_mail,
            ImapFetchedHeader {
                mailbox_id: all_mail.mailbox_id.clone(),
                uid: ImapUid(44),
                modseq: Some(ImapModSeq(902)),
                flags: vec!["\\Seen".to_string(), "\\Flagged".to_string()],
                rfc822_size: 512,
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Flag delta\r\nMessage-ID: <gmail-flag-delta@example.test>\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:01:00Z".to_string(),
            },
        )
        .expect("all mail header");
        let message_id =
            ImapAdapterProviderProfile::gmail().canonical_message_id(&all_mail_header.message);
        let inbox_location = ImapMessageLocation {
            message_id: message_id.clone(),
            mailbox_id: MailboxId::from("imap:mailbox:inbox"),
            uid_validity: ImapUidValidity(7),
            uid: ImapUid(12),
            modseq: Some(ImapModSeq(600)),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };
        let all_mail_location = ImapMessageLocation {
            message_id: message_id.clone(),
            ..all_mail_header.location.clone()
        };

        let batch = imap_condstore_delta_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::from_tokens(["IMAP4rev1", "QRESYNC", "X-GM-EXT-1"]),
                mailboxes: vec![
                    map_imap_mailbox("INBOX", ["\\Inbox"]),
                    map_imap_mailbox("[Gmail]/All Mail", ["\\All"]),
                ],
            },
            vec![all_mail_header],
            Vec::new(),
            vec![inbox_location.clone(), all_mail_location],
            Vec::new(),
            "2026-04-25T00:01:00Z".to_string(),
        );

        assert_eq!(batch.messages.len(), 1);
        assert_eq!(batch.messages[0].id, message_id);
        assert_eq!(batch.messages[0].keywords, vec!["$flagged", "$seen"]);
        assert_eq!(
            batch.messages[0]
                .mailbox_ids
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                inbox_location.mailbox_id,
                MailboxId::from("imap:mailbox:all"),
            ])
        );
        assert!(batch.deleted_imap_message_locations.is_empty());
        assert!(batch.deleted_message_ids.is_empty());
    }

    // spec: docs/L0-testing#provider-observation-contracts
    #[test]
    fn gmail_starred_location_removal_unflags_remaining_message() {
        let all_mail = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:all"),
            mailbox_name: "[Gmail]/All Mail".to_string(),
            uid_validity: ImapUidValidity(8),
            uid_next: None,
            highest_modseq: Some(ImapModSeq(902)),
        };
        let all_mail_header = imap_header_message_record(
            &all_mail,
            ImapFetchedHeader {
                mailbox_id: all_mail.mailbox_id.clone(),
                uid: ImapUid(44),
                modseq: Some(ImapModSeq(902)),
                flags: vec!["\\Seen".to_string()],
                rfc822_size: 512,
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Flag delta\r\nMessage-ID: <gmail-unflag-delta@example.test>\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:01:00Z".to_string(),
            },
        )
        .expect("all mail header");
        let message_id =
            ImapAdapterProviderProfile::gmail().canonical_message_id(&all_mail_header.message);
        let starred_location = ImapMessageLocation {
            message_id: message_id.clone(),
            mailbox_id: MailboxId::from("imap:mailbox:starred"),
            uid_validity: ImapUidValidity(7),
            uid: ImapUid(12),
            modseq: Some(ImapModSeq(600)),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };
        let all_mail_location = ImapMessageLocation {
            message_id: message_id.clone(),
            ..all_mail_header.location.clone()
        };

        let batch = imap_condstore_delta_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::from_tokens(["IMAP4rev1", "QRESYNC", "X-GM-EXT-1"]),
                mailboxes: vec![
                    map_imap_mailbox("[Gmail]/Starred", ["\\Flagged"]),
                    map_imap_mailbox("[Gmail]/All Mail", ["\\All"]),
                ],
            },
            vec![all_mail_header],
            Vec::new(),
            vec![starred_location.clone(), all_mail_location],
            vec![(
                starred_location.mailbox_id.clone(),
                starred_location.uid_validity,
                starred_location.uid,
            )],
            "2026-04-25T00:01:00Z".to_string(),
        );

        assert_eq!(batch.messages.len(), 1);
        assert_eq!(batch.messages[0].id, message_id);
        assert_eq!(batch.messages[0].keywords, vec!["$seen"]);
        assert_eq!(
            batch.messages[0].mailbox_ids,
            vec![MailboxId::from("imap:mailbox:all")]
        );
        assert_eq!(
            batch.deleted_imap_message_locations,
            vec![starred_location.key()]
        );
        assert!(batch.deleted_message_ids.is_empty());
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
                has_attachment: false,
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
            batch.deleted_imap_message_locations,
            vec![vanished_location.key()]
        );
        assert_eq!(
            batch.deleted_message_ids,
            vec![vanished_location.message_id]
        );
        assert!(!batch
            .deleted_message_ids
            .contains(&unchanged_location.message_id));
    }

    // spec: docs/L0-testing#provider-observation-contracts
    #[test]
    fn qresync_vanished_deduplicates_canonical_message_deletions() {
        let message_id = MessageId::from("imap:gmail:rfc822msgid:676d61696c2d31");
        let inbox_location = ImapMessageLocation {
            message_id: message_id.clone(),
            mailbox_id: MailboxId::from("imap:mailbox:inbox"),
            uid_validity: ImapUidValidity(7),
            uid: ImapUid(12),
            modseq: Some(ImapModSeq(600)),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };
        let all_mail_location = ImapMessageLocation {
            message_id: message_id.clone(),
            mailbox_id: MailboxId::from("imap:mailbox:all"),
            uid_validity: ImapUidValidity(8),
            uid: ImapUid(44),
            modseq: Some(ImapModSeq(601)),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };

        let batch = imap_condstore_delta_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::from_tokens(["IMAP4rev1", "QRESYNC"]),
                mailboxes: vec![
                    map_imap_mailbox("INBOX", ["\\Inbox"]),
                    map_imap_mailbox("[Gmail]/All Mail", ["\\All"]),
                ],
            },
            Vec::new(),
            Vec::new(),
            vec![inbox_location.clone(), all_mail_location.clone()],
            vec![
                (
                    inbox_location.mailbox_id.clone(),
                    inbox_location.uid_validity,
                    inbox_location.uid,
                ),
                (
                    all_mail_location.mailbox_id.clone(),
                    all_mail_location.uid_validity,
                    all_mail_location.uid,
                ),
            ],
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert_eq!(
            batch.deleted_imap_message_locations,
            vec![inbox_location.key(), all_mail_location.key()]
        );
        assert_eq!(batch.deleted_message_ids, vec![message_id]);
    }

    // spec: docs/L0-testing#provider-observation-contracts
    #[test]
    fn qresync_single_location_vanish_preserves_canonical_message() {
        let message_id = MessageId::from("imap:gmail:rfc822msgid:676d61696c2d31");
        let inbox_location = ImapMessageLocation {
            message_id: message_id.clone(),
            mailbox_id: MailboxId::from("imap:mailbox:inbox"),
            uid_validity: ImapUidValidity(7),
            uid: ImapUid(12),
            modseq: Some(ImapModSeq(600)),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };
        let all_mail_location = ImapMessageLocation {
            message_id,
            mailbox_id: MailboxId::from("imap:mailbox:all"),
            uid_validity: ImapUidValidity(8),
            uid: ImapUid(44),
            modseq: Some(ImapModSeq(601)),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };

        let batch = imap_condstore_delta_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::from_tokens(["IMAP4rev1", "QRESYNC"]),
                mailboxes: vec![
                    map_imap_mailbox("INBOX", ["\\Inbox"]),
                    map_imap_mailbox("[Gmail]/All Mail", ["\\All"]),
                ],
            },
            Vec::new(),
            Vec::new(),
            vec![inbox_location.clone(), all_mail_location],
            vec![(
                inbox_location.mailbox_id.clone(),
                inbox_location.uid_validity,
                inbox_location.uid,
            )],
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert_eq!(
            batch.deleted_imap_message_locations,
            vec![inbox_location.key()]
        );
        assert!(batch.deleted_message_ids.is_empty());
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
