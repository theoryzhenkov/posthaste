use super::*;

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
            partial_initial_uid: None,
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
        partial_initial_uid: None,
        updated_at: "2026-04-24T00:00:00Z".to_string(),
    };
    let snapshot = ImapChangedSinceSnapshot {
        selected,
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
