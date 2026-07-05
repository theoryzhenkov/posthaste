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

/// DS1 mail-loss audit (IMAP path): the ONLY IMAP batch that sets
/// `replace_all_messages` (which drives prune-by-absence) is the full snapshot,
/// and its remote message set is EXACTLY the complete header set the caller
/// built from a `UID SEARCH UNDELETED` to exhaustion (IMAP returns all matching
/// UIDs in one untagged response — no server-side cap like JMAP's) with every
/// UID fetched (per-mailbox errors abort the whole batch via `?`). This asserts
/// the prune set is the complete fetched set, never a partial/capped subset.
#[test]
fn full_snapshot_prune_set_is_the_complete_fetched_header_set() {
    let selected = ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: ImapUidValidity(9),
        uid_next: None,
        highest_modseq: None,
    };
    let mapped: Vec<_> = [10u32, 20, 30]
        .into_iter()
        .map(|uid| {
            imap_header_message_record(
                &selected,
                ImapFetchedHeader {
                    mailbox_id: selected.mailbox_id.clone(),
                    uid: ImapUid(uid),
                    modseq: Some(ImapModSeq(700 + u64::from(uid))),
                    flags: Vec::new(),
                    rfc822_size: 512,
                    has_attachment: false,
                    headers: format!(
                        "From: Alice <alice@example.test>\r\nSubject: Hello {uid}\r\n\r\n"
                    )
                    .into_bytes(),
                    updated_at: "2026-04-25T00:00:00Z".to_string(),
                },
            )
            .expect("mapped header")
        })
        .collect();
    let expected_ids: std::collections::BTreeSet<_> =
        mapped.iter().map(|m| m.message.id.clone()).collect();

    let batch = imap_full_sync_batch(
        &AccountId::from("primary"),
        DiscoveredImapAccount {
            capabilities: ImapCapabilities::default(),
            mailboxes: vec![map_imap_mailbox("INBOX", ["\\Inbox"])],
        },
        mapped,
        vec![ImapMailboxSyncState {
            mailbox_id: selected.mailbox_id,
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(9),
            highest_uid: Some(ImapUid(30)),
            highest_modseq: Some(ImapModSeq(730)),
            partial_initial_uid: None,
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        }],
        "2026-04-25T00:00:00Z".to_string(),
    );

    assert!(batch.replace_all_messages);
    let batch_ids: std::collections::BTreeSet<_> =
        batch.messages.iter().map(|m| m.id.clone()).collect();
    assert_eq!(
        batch_ids, expected_ids,
        "the full-snapshot prune set must equal the complete fetched header set",
    );
}

/// B4 / DS1: a resumable initial-snapshot CHUNK is additive, never destructive.
/// It carries this chunk's rows + the advancing checkpoint and must NEVER set
/// `replace_all_messages` (which would drive prune-by-absence against an
/// incomplete remote set), carry deletions, or re-emit mailboxes.
#[test]
fn initial_snapshot_chunk_is_upsert_only_and_carries_the_checkpoint() {
    let selected = ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: ImapUidValidity(9),
        uid_next: None,
        highest_modseq: None,
    };
    let mapped: Vec<_> = [10u32, 20]
        .into_iter()
        .map(|uid| {
            imap_header_message_record(
                &selected,
                ImapFetchedHeader {
                    mailbox_id: selected.mailbox_id.clone(),
                    uid: ImapUid(uid),
                    modseq: Some(ImapModSeq(700 + u64::from(uid))),
                    flags: Vec::new(),
                    rfc822_size: 512,
                    has_attachment: false,
                    headers: format!(
                        "From: Alice <alice@example.test>\r\nSubject: Hello {uid}\r\n\r\n"
                    )
                    .into_bytes(),
                    updated_at: "2026-04-25T00:00:00Z".to_string(),
                },
            )
            .expect("mapped header")
        })
        .collect();

    let checkpoint = ImapMailboxSyncState {
        mailbox_id: selected.mailbox_id,
        mailbox_name: "INBOX".to_string(),
        uid_validity: ImapUidValidity(9),
        highest_uid: None,
        highest_modseq: None,
        partial_initial_uid: Some(ImapUid(20)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };

    let batch = crate::imap_initial_snapshot_chunk_batch(
        &DiscoveredImapAccount {
            capabilities: ImapCapabilities::default(),
            mailboxes: vec![map_imap_mailbox("INBOX", ["\\Inbox"])],
        },
        mapped,
        checkpoint.clone(),
    );

    assert!(
        !batch.replace_all_messages,
        "a mid-sync checkpoint must never drive prune-by-absence (DS1)",
    );
    assert!(!batch.replace_all_mailboxes);
    assert!(batch.deleted_message_ids.is_empty());
    assert!(batch.deleted_imap_message_locations.is_empty());
    assert!(
        batch.mailboxes.is_empty(),
        "mailboxes are emitted separately"
    );
    assert_eq!(batch.messages.len(), 2, "this chunk's rows are upserted");
    assert_eq!(batch.imap_message_locations.len(), 2);
    assert_eq!(
        batch.imap_mailbox_states,
        vec![checkpoint],
        "the advancing resume cursor is committed with the chunk",
    );
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
            mailbox_id: selected.mailbox_id,
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(9),
            highest_uid: Some(ImapUid(42)),
            highest_modseq: Some(ImapModSeq(777)),
            partial_initial_uid: None,
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
