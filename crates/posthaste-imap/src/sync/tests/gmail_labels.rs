use super::*;

// spec: docs/L0-providers#gmail-label-observation
#[test]
fn gmail_full_sync_uses_x_gm_labels_for_mailbox_membership_and_starred_keyword() {
    let all_mail = ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:5b476d61696c5d2f416c6c204d61696c"),
        mailbox_name: "[Gmail]/All Mail".to_string(),
        uid_validity: ImapUidValidity(8),
        uid_next: None,
        highest_modseq: None,
    };
    let header = imap_header_message_record_with_gmail_metadata(
        &all_mail,
        ImapFetchedHeader {
            mailbox_id: all_mail.mailbox_id.clone(),
            uid: ImapUid(88),
            modseq: None,
            flags: vec!["\\Seen".to_string()],
            rfc822_size: 512,
            has_attachment: false,
            headers: b"From: Alice <alice@example.test>\r\nSubject: Labels\r\nMessage-ID: <gmail-labels@example.test>\r\n\r\n".to_vec(),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        },
        ImapGmailMetadata {
            message_id: Some(GmailMessageId(1278455344230334865)),
            thread_id: Some(GmailThreadId(1266894439832287888)),
            labels_observed: true,
            labels: vec![
                "\\Inbox".into(),
                "\\Starred".into(),
                "Important".into(),
                "Project Alpha".into(),
            ],
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
                map_imap_mailbox("[Gmail]/Important", ["\\Important"]),
                map_imap_mailbox("[Gmail]/Starred", ["\\Flagged"]),
                map_imap_mailbox("Project Alpha", ["\\HasNoChildren"]),
            ],
        },
        vec![header],
        Vec::new(),
        "2026-04-25T00:00:00Z".to_string(),
    );

    assert_eq!(batch.messages.len(), 1);
    assert_eq!(
        batch.messages[0].mailbox_ids,
        vec![
            MailboxId::from("imap:mailbox:494e424f58"),
            MailboxId::from("imap:mailbox:50726f6a65637420416c706861"),
            MailboxId::from("imap:mailbox:5b476d61696c5d2f416c6c204d61696c"),
            MailboxId::from("imap:mailbox:5b476d61696c5d2f496d706f7274616e74"),
            MailboxId::from("imap:mailbox:5b476d61696c5d2f53746172726564"),
        ]
    );
    assert_eq!(batch.messages[0].keywords, vec!["$flagged", "$seen"]);
    assert_eq!(
        batch.imap_message_locations[0].mailbox_id,
        all_mail.mailbox_id
    );
}

// spec: docs/L0-providers#gmail-label-observation
#[test]
fn gmail_label_observation_removes_stale_inbox_and_starred_state() {
    let all_mail = ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:5b476d61696c5d2f416c6c204d61696c"),
        mailbox_name: "[Gmail]/All Mail".to_string(),
        uid_validity: ImapUidValidity(8),
        uid_next: None,
        highest_modseq: Some(ImapModSeq(902)),
    };
    let header = imap_header_message_record_with_gmail_metadata(
        &all_mail,
        ImapFetchedHeader {
            mailbox_id: all_mail.mailbox_id.clone(),
            uid: ImapUid(44),
            modseq: Some(ImapModSeq(902)),
            flags: vec!["\\Seen".to_string(), "\\Flagged".to_string()],
            rfc822_size: 512,
            has_attachment: false,
            headers: b"From: Alice <alice@example.test>\r\nSubject: Archived\r\nMessage-ID: <gmail-archived@example.test>\r\n\r\n".to_vec(),
            updated_at: "2026-04-25T00:01:00Z".to_string(),
        },
        ImapGmailMetadata {
            message_id: Some(GmailMessageId(998877)),
            thread_id: Some(GmailThreadId(112233)),
            labels_observed: true,
            labels: Vec::new(),
        },
    )
    .expect("all mail header");
    let message_id = MessageId::from("imap:gmail:msgid:998877");
    let inbox_location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
        uid_validity: ImapUidValidity(7),
        uid: ImapUid(12),
        modseq: Some(ImapModSeq(600)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    let starred_location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: MailboxId::from("imap:mailbox:5b476d61696c5d2f53746172726564"),
        uid_validity: ImapUidValidity(9),
        uid: ImapUid(13),
        modseq: Some(ImapModSeq(601)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    let all_mail_location = ImapMessageLocation {
        message_id: message_id.clone(),
        ..header.location.clone()
    };
    let stale_location_keys = vec![inbox_location.key(), starred_location.key()];

    let batch = imap_condstore_delta_sync_batch(
        &AccountId::from("primary"),
        DiscoveredImapAccount {
            capabilities: ImapCapabilities::from_tokens(["IMAP4rev1", "QRESYNC", "X-GM-EXT-1"]),
            mailboxes: vec![
                map_imap_mailbox("INBOX", ["\\Inbox"]),
                map_imap_mailbox("[Gmail]/All Mail", ["\\All"]),
                map_imap_mailbox("[Gmail]/Starred", ["\\Flagged"]),
            ],
        },
        vec![header],
        Vec::new(),
        vec![
            inbox_location.clone(),
            starred_location.clone(),
            all_mail_location,
        ],
        Vec::new(),
        "2026-04-25T00:01:00Z".to_string(),
    );

    assert_eq!(batch.messages.len(), 1);
    assert_eq!(
        batch.messages[0].mailbox_ids,
        vec![MailboxId::from(
            "imap:mailbox:5b476d61696c5d2f416c6c204d61696c"
        )]
    );
    assert_eq!(batch.messages[0].keywords, vec!["$seen"]);
    assert_eq!(batch.deleted_imap_message_locations, stale_location_keys);
    assert!(batch.deleted_message_ids.is_empty());
}

// spec: docs/L0-testing#provider-observation-contracts
#[test]
fn gmail_label_observation_preserves_all_mail_location_without_all_label() {
    let inbox = ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: ImapUidValidity(7),
        uid_next: None,
        highest_modseq: Some(ImapModSeq(902)),
    };
    let header = imap_header_message_record_with_gmail_metadata(
        &inbox,
        ImapFetchedHeader {
            mailbox_id: inbox.mailbox_id.clone(),
            uid: ImapUid(44),
            modseq: Some(ImapModSeq(902)),
            flags: vec!["\\Seen".to_string()],
            rfc822_size: 512,
            has_attachment: false,
            headers: b"From: Alice <alice@example.test>\r\nSubject: Inbox\r\nMessage-ID: <gmail-inbox@example.test>\r\n\r\n".to_vec(),
            updated_at: "2026-04-25T00:01:00Z".to_string(),
        },
        ImapGmailMetadata {
            message_id: Some(GmailMessageId(887766)),
            thread_id: None,
            labels_observed: true,
            labels: vec!["\\Inbox".into()],
        },
    )
    .expect("inbox header");
    let message_id = MessageId::from("imap:gmail:msgid:887766");
    let inbox_location = ImapMessageLocation {
        message_id: message_id.clone(),
        ..header.location.clone()
    };
    let all_mail_location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: MailboxId::from("imap:mailbox:5b476d61696c5d2f416c6c204d61696c"),
        uid_validity: ImapUidValidity(8),
        uid: ImapUid(88),
        modseq: Some(ImapModSeq(800)),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };

    let batch = imap_condstore_delta_sync_batch(
        &AccountId::from("primary"),
        DiscoveredImapAccount {
            capabilities: ImapCapabilities::from_tokens(["IMAP4rev1", "QRESYNC", "X-GM-EXT-1"]),
            mailboxes: vec![
                map_imap_mailbox("INBOX", ["\\Inbox"]),
                map_imap_mailbox("[Gmail]/All Mail", ["\\All"]),
                map_imap_mailbox("[Gmail]/Starred", ["\\Flagged"]),
            ],
        },
        vec![header],
        Vec::new(),
        vec![inbox_location, all_mail_location],
        Vec::new(),
        "2026-04-25T00:01:00Z".to_string(),
    );

    assert_eq!(batch.messages.len(), 1);
    assert_eq!(
        batch.messages[0].mailbox_ids,
        vec![
            MailboxId::from("imap:mailbox:494e424f58"),
            MailboxId::from("imap:mailbox:5b476d61696c5d2f416c6c204d61696c"),
        ]
    );
    assert_eq!(batch.messages[0].keywords, vec!["$seen"]);
    assert!(batch.deleted_imap_message_locations.is_empty());
    assert!(batch.deleted_message_ids.is_empty());
}

// spec: docs/L0-providers#gmail-label-observation
#[test]
fn gmail_custom_label_without_discovered_mailbox_does_not_become_keyword() {
    let all_mail = ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:5b476d61696c5d2f416c6c204d61696c"),
        mailbox_name: "[Gmail]/All Mail".to_string(),
        uid_validity: ImapUidValidity(8),
        uid_next: None,
        highest_modseq: None,
    };
    let header = imap_header_message_record_with_gmail_metadata(
        &all_mail,
        ImapFetchedHeader {
            mailbox_id: all_mail.mailbox_id.clone(),
            uid: ImapUid(88),
            modseq: None,
            flags: Vec::new(),
            rfc822_size: 512,
            has_attachment: false,
            headers: b"From: Alice <alice@example.test>\r\nSubject: Unknown Label\r\nMessage-ID: <gmail-unknown-label@example.test>\r\n\r\n".to_vec(),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        },
        ImapGmailMetadata {
            message_id: Some(GmailMessageId(556677)),
            thread_id: None,
            labels_observed: true,
            labels: vec!["Client With Spaces".into()],
        },
    )
    .expect("all mail header");

    let batch = imap_full_sync_batch(
        &AccountId::from("primary"),
        DiscoveredImapAccount {
            capabilities: ImapCapabilities::from_tokens(["IMAP4rev1", "X-GM-EXT-1"]),
            mailboxes: vec![map_imap_mailbox("[Gmail]/All Mail", ["\\All"])],
        },
        vec![header],
        Vec::new(),
        "2026-04-25T00:00:00Z".to_string(),
    );

    assert_eq!(
        batch.messages[0].mailbox_ids,
        vec![MailboxId::from(
            "imap:mailbox:5b476d61696c5d2f416c6c204d61696c"
        )]
    );
    assert!(batch.messages[0].keywords.is_empty());
}
