use super::*;

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
