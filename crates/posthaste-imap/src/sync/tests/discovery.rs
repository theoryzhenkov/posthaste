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
