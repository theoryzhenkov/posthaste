use super::*;

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
fn gmail_sync_prefers_x_gm_msgid_over_differing_or_missing_rfc_message_id() {
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
    let gmail_message_id = GmailMessageId(1278455344230334865);
    let inbox_header = imap_header_message_record_with_gmail_metadata(
        &inbox,
        ImapFetchedHeader {
            mailbox_id: inbox.mailbox_id.clone(),
            uid: ImapUid(12),
            modseq: None,
            flags: vec!["\\Seen".to_string()],
            rfc822_size: 512,
            has_attachment: false,
            headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\nMessage-ID: <bad-one@example.test>\r\n\r\n".to_vec(),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        },
        ImapGmailMetadata {
            message_id: Some(gmail_message_id),
            thread_id: Some(GmailThreadId(1266894439832287888)),
            labels_observed: false,
            labels: vec!["INBOX".into()],
        },
    )
    .expect("inbox header");
    let all_mail_header = imap_header_message_record_with_gmail_metadata(
        &all_mail,
        ImapFetchedHeader {
            mailbox_id: all_mail.mailbox_id.clone(),
            uid: ImapUid(88),
            modseq: None,
            flags: vec!["\\Flagged".to_string()],
            rfc822_size: 512,
            has_attachment: false,
            headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\n\r\n".to_vec(),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        },
        ImapGmailMetadata {
            message_id: Some(gmail_message_id),
            thread_id: Some(GmailThreadId(1266894439832287888)),
            labels_observed: false,
            labels: vec!["\\All".into()],
        },
    )
    .expect("all mail header");

    let batch = imap_full_sync_batch(
        &AccountId::from("primary"),
        DiscoveredImapAccount {
            capabilities: ImapCapabilities::from_tokens(["IMAP4rev1", "X-GM-EXT-1"]),
            mailboxes: vec![
                map_imap_mailbox("INBOX", ["\\Inbox"]),
                map_imap_mailbox("[Gmail]/All Mail", ["\\All"]),
            ],
        },
        vec![inbox_header, all_mail_header],
        Vec::new(),
        "2026-04-25T00:00:00Z".to_string(),
    );

    assert_eq!(batch.messages.len(), 1);
    assert_eq!(
        batch.messages[0].id.as_str(),
        "imap:gmail:msgid:1278455344230334865"
    );
    assert_eq!(
        batch.messages[0].source_thread_id.as_str(),
        "imap:gmail:thrid:1266894439832287888"
    );
    assert_eq!(
        batch.messages[0].mailbox_ids,
        vec![
            MailboxId::from("imap:mailbox:all"),
            MailboxId::from("imap:mailbox:inbox")
        ]
    );
    assert_eq!(batch.messages[0].keywords, vec!["$flagged", "$seen"]);
    assert_eq!(batch.imap_message_locations.len(), 2);
    assert!(batch.imap_message_locations.iter().any(|location| {
        location.mailbox_id == inbox.mailbox_id
            && location.uid_validity == ImapUidValidity(7)
            && location.uid == ImapUid(12)
    }));
    assert!(batch.imap_message_locations.iter().any(|location| {
        location.mailbox_id == all_mail.mailbox_id
            && location.uid_validity == ImapUidValidity(8)
            && location.uid == ImapUid(88)
    }));
}
