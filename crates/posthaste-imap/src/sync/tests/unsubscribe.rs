//! List-Unsubscribe (RFC 2369) + List-Unsubscribe-Post (RFC 8058) extraction
//! at IMAP header ingest, and its re-extraction on the lazy body fetch (the
//! old-mail backfill path).

use super::*;

fn selected_inbox() -> ImapSelectedMailbox {
    ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:inbox"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: ImapUidValidity(7),
        uid_next: None,
        highest_modseq: None,
    }
}

fn fetched_with_headers(headers: &[u8]) -> ImapFetchedHeader {
    ImapFetchedHeader {
        mailbox_id: MailboxId::from("imap:mailbox:inbox"),
        uid: ImapUid(12),
        modseq: None,
        flags: Vec::new(),
        rfc822_size: 512,
        has_attachment: false,
        headers: headers.to_vec(),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    }
}

#[test]
fn header_ingest_parses_one_click_unsubscribe_targets() {
    let mapped = imap_header_message_record(
        &selected_inbox(),
        fetched_with_headers(
            b"From: News <news@example.test>\r\n\
              Subject: Weekly digest\r\n\
              List-Unsubscribe: <https://news.example.test/unsub/opaque>,\r\n \
              <mailto:unsub@example.test?subject=stop>\r\n\
              List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\r\n",
        ),
    )
    .expect("mapped header");

    let targets = mapped
        .message
        .list_unsubscribe
        .expect("targets parsed at ingest");
    assert_eq!(
        targets.https.as_deref(),
        Some("https://news.example.test/unsub/opaque")
    );
    assert_eq!(
        targets.mailto.as_deref(),
        Some("mailto:unsub@example.test?subject=stop")
    );
    assert!(targets.one_click);
}

#[test]
fn header_ingest_without_post_header_is_not_one_click() {
    let mapped = imap_header_message_record(
        &selected_inbox(),
        fetched_with_headers(
            b"From: News <news@example.test>\r\n\
              Subject: Weekly digest\r\n\
              List-Unsubscribe: <mailto:unsub@example.test>\r\n\r\n",
        ),
    )
    .expect("mapped header");

    let targets = mapped.message.list_unsubscribe.expect("targets parsed");
    assert_eq!(targets.https, None);
    assert_eq!(targets.mailto.as_deref(), Some("mailto:unsub@example.test"));
    assert!(!targets.one_click);
}

#[test]
fn header_ingest_without_the_header_or_with_junk_stores_none() {
    for headers in [
        b"From: Alice <alice@example.test>\r\nSubject: Hello\r\n\r\n".as_slice(),
        b"From: Alice <alice@example.test>\r\nList-Unsubscribe: junk, no brackets\r\n\r\n"
            .as_slice(),
        // http-only target: dropped, never downgraded-to.
        b"From: Alice <alice@example.test>\r\nList-Unsubscribe: <http://x.example.test/u>\r\n\r\n"
            .as_slice(),
    ] {
        let mapped = imap_header_message_record(&selected_inbox(), fetched_with_headers(headers))
            .expect("mapped header");
        assert_eq!(mapped.message.list_unsubscribe, None);
    }
}

#[test]
fn body_fetch_re_extracts_unsubscribe_targets_for_backfill() {
    let raw = b"From: News <news@example.test>\r\n\
                Subject: Weekly digest\r\n\
                List-Unsubscribe: <https://news.example.test/unsub/opaque>\r\n\
                List-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\
                Content-Type: text/plain\r\n\r\n\
                Hello\r\n"
        .to_vec();
    let body =
        crate::imap_body_from_raw_mime(&MessageId::from("message-1"), raw).expect("body parsed");

    let targets = body
        .list_unsubscribe
        .expect("targets re-extracted from the raw MIME");
    assert_eq!(
        targets.https.as_deref(),
        Some("https://news.example.test/unsub/opaque")
    );
    assert!(targets.one_click);
}
