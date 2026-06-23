use posthaste_domain::{GmailMessageId, GmailThreadId, ImapUidValidity, MailboxId};

use super::*;

#[test]
fn maps_header_metadata_without_fetching_body() {
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
            flags: vec!["\\Seen".to_string(), "\\Flagged".to_string()],
            rfc822_size: 512,
            has_attachment: false,
            headers: b"From: Alice <alice@example.test>\r\nDate: Sat, 20 Nov 2021 14:22:01 -0800\r\nSubject: Hello\r\nMessage-ID: <m1@example.test>\r\nReferences: <root@example.test> <parent@example.test>\r\n\r\n".to_vec(),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        },
    )
    .expect("mapped header");

    assert_eq!(
        mapped.message.id.as_str(),
        "imap:9:42:696d61703a6d61696c626f783a34393465343234663538"
    );
    assert_eq!(mapped.message.subject.as_deref(), Some("Hello"));
    assert_eq!(mapped.message.from_name.as_deref(), Some("Alice"));
    assert_eq!(
        mapped.message.from_email.as_deref(),
        Some("alice@example.test")
    );
    assert_eq!(
        mapped.message.received_at,
        "2021-11-20T14:22:01-08:00".to_string()
    );
    assert_eq!(mapped.message.keywords, vec!["$flagged", "$seen"]);
    assert_eq!(mapped.message.body_text, None);
    assert_eq!(mapped.message.raw_mime, None);
    assert_eq!(mapped.location.uid, ImapUid(42));
    assert_eq!(mapped.location.modseq, Some(ImapModSeq(777)));
}

#[test]
fn maps_typed_gmail_identity_and_preserves_imap_location_addressability() {
    let selected = ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: ImapUidValidity(9),
        uid_next: None,
        highest_modseq: None,
    };

    let mapped = imap_header_message_record_with_gmail_metadata(
        &selected,
        ImapFetchedHeader {
            mailbox_id: selected.mailbox_id.clone(),
            uid: ImapUid(42),
            modseq: Some(ImapModSeq(777)),
            flags: Vec::new(),
            rfc822_size: 512,
            has_attachment: false,
            headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\nMessage-ID: <unstable@example.test>\r\n\r\n".to_vec(),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        },
        ImapGmailMetadata {
            message_id: Some(GmailMessageId(1278455344230334865)),
            thread_id: Some(GmailThreadId(1266894439832287888)),
            labels_observed: true,
            labels: vec!["INBOX".into(), "\\Important".into()],
        },
    )
    .expect("mapped Gmail header");

    assert_eq!(
        mapped.message.id.as_str(),
        "imap:gmail:msgid:1278455344230334865"
    );
    assert_eq!(
        mapped.message.source_thread_id.as_str(),
        "imap:gmail:thrid:1266894439832287888"
    );
    assert_eq!(
        mapped.message.rfc_message_id.as_deref(),
        Some("unstable@example.test")
    );
    assert_eq!(mapped.location.mailbox_id, selected.mailbox_id);
    assert_eq!(mapped.location.uid_validity, ImapUidValidity(9));
    assert_eq!(mapped.location.uid, ImapUid(42));
    assert_eq!(mapped.location.message_id, mapped.message.id);
}

#[test]
fn malformed_headers_return_typed_error() {
    let selected = ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: ImapUidValidity(9),
        uid_next: None,
        highest_modseq: None,
    };

    let error = imap_header_message_record(
        &selected,
        ImapFetchedHeader {
            mailbox_id: selected.mailbox_id.clone(),
            uid: ImapUid(42),
            modseq: None,
            flags: Vec::new(),
            rfc822_size: 0,
            has_attachment: false,
            headers: Vec::new(),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        },
    )
    .expect_err("empty headers are invalid");

    assert!(matches!(error, ImapAdapterError::ParseMessageHeaders));
}

#[test]
fn maps_custom_imap_keywords_to_jmap_keywords() {
    let keywords = imap_flag_keywords(&[
        "\\Seen".to_string(),
        IMAP_FLAG_FORWARDED.to_string(),
        "project-x".to_string(),
        "\\UnknownExtension".to_string(),
    ]);

    assert_eq!(
        keywords,
        vec![
            SystemKeyword::Forwarded.as_str(),
            SystemKeyword::Seen.as_str(),
            "project-x"
        ]
    );
}

#[test]
fn parses_the_draft_id_header_into_the_message_record() {
    let selected = ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
        mailbox_name: "Drafts".to_string(),
        uid_validity: ImapUidValidity(9),
        uid_next: None,
        highest_modseq: None,
    };
    let mapped = imap_header_message_record(
        &selected,
        ImapFetchedHeader {
            mailbox_id: selected.mailbox_id.clone(),
            uid: ImapUid(7),
            modseq: None,
            flags: vec!["\\Draft".to_string()],
            rfc822_size: 128,
            has_attachment: false,
            headers: b"X-Posthaste-Draft-Id: draft-local-stable\r\nFrom: Alice <alice@example.test>\r\nSubject: WIP\r\n\r\n".to_vec(),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        },
    )
    .expect("mapped header");

    assert_eq!(
        mapped.message.draft_id.as_deref(),
        Some("draft-local-stable")
    );
}

#[test]
fn no_draft_id_when_the_header_is_absent() {
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
            uid: ImapUid(8),
            modseq: None,
            flags: Vec::new(),
            rfc822_size: 64,
            has_attachment: false,
            headers: b"From: Alice <alice@example.test>\r\nSubject: Hi\r\n\r\n".to_vec(),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        },
    )
    .expect("mapped header");

    assert_eq!(mapped.message.draft_id, None);
}
