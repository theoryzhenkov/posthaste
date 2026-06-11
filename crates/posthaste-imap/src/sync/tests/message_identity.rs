use super::*;

#[test]
fn gmail_sync_keeps_same_bad_rfc_message_id_separate_when_x_gm_msgid_differs() {
    let inbox = ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:inbox"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: ImapUidValidity(7),
        uid_next: None,
        highest_modseq: None,
    };
    let all_mail = ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:all"),
        mailbox_name: "[Gmail]/All Mail".to_string(),
        uid_validity: ImapUidValidity(8),
        uid_next: None,
        highest_modseq: None,
    };
    let first = imap_header_message_record_with_gmail_metadata(
        &inbox,
        ImapFetchedHeader {
            mailbox_id: inbox.mailbox_id.clone(),
            uid: ImapUid(12),
            modseq: None,
            flags: Vec::new(),
            rfc822_size: 512,
            has_attachment: false,
            headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\nMessage-ID: <bad-duplicate@example.test>\r\n\r\n".to_vec(),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        },
        ImapGmailMetadata {
            message_id: Some(GmailMessageId(111)),
            thread_id: None,
            labels_observed: false,
            labels: Vec::new(),
        },
    )
    .expect("first header");
    let second = imap_header_message_record_with_gmail_metadata(
        &all_mail,
        ImapFetchedHeader {
            mailbox_id: all_mail.mailbox_id.clone(),
            uid: ImapUid(88),
            modseq: None,
            flags: Vec::new(),
            rfc822_size: 512,
            has_attachment: false,
            headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\nMessage-ID: <bad-duplicate@example.test>\r\n\r\n".to_vec(),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        },
        ImapGmailMetadata {
            message_id: Some(GmailMessageId(222)),
            thread_id: None,
            labels_observed: false,
            labels: Vec::new(),
        },
    )
    .expect("second header");

    let batch = imap_full_sync_batch(
        &AccountId::from("primary"),
        DiscoveredImapAccount {
            capabilities: ImapCapabilities::from_tokens(["IMAP4rev1", "X-GM-EXT-1"]),
            mailboxes: vec![
                map_imap_mailbox("INBOX", ["\\Inbox"]),
                map_imap_mailbox("[Gmail]/All Mail", ["\\All"]),
            ],
        },
        vec![first, second],
        Vec::new(),
        "2026-04-25T00:00:00Z".to_string(),
    );

    assert_eq!(batch.messages.len(), 2);
    assert_eq!(batch.messages[0].id.as_str(), "imap:gmail:msgid:111");
    assert_eq!(batch.messages[1].id.as_str(), "imap:gmail:msgid:222");
    assert_eq!(batch.imap_message_locations.len(), 2);
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
