use super::*;

use crate::ImapMailboxHeaderSnapshot;

/// Regression: an empty (or canonically-deduped) mailbox fetches no
/// MODSEQ-bearing headers on a full snapshot, so the stored watermark MUST come
/// from the SELECT/EXAMINE `[HIGHESTMODSEQ]` (RFC 7162). Deriving it from the
/// fetched headers alone stored `None`, which left the mailbox unable to take
/// the CONDSTORE/QRESYNC delta path — re-running a full snapshot on every sync
/// (the "Gmail fetches every mailbox each sync" symptom).
#[test]
fn header_snapshot_state_uses_select_highestmodseq_for_an_empty_mailbox() {
    let selected = ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:5b476d61696c5d2f5370616d"),
        mailbox_name: "[Gmail]/Spam".to_string(),
        uid_validity: ImapUidValidity(7),
        uid_next: None,
        highest_modseq: Some(ImapModSeq(4242)),
    };
    let snapshot = ImapMailboxHeaderSnapshot {
        selected,
        headers: Vec::new(),
    };

    let state =
        imap_mailbox_state_from_header_snapshot(&snapshot, "2026-06-29T00:00:00Z".to_string());

    assert_eq!(
        state.highest_modseq,
        Some(ImapModSeq(4242)),
        "an empty mailbox must persist the SELECT HIGHESTMODSEQ so it can delta next sync",
    );
}

/// Without a SELECT `[HIGHESTMODSEQ]` (non-CONDSTORE-in-SELECT servers), fall
/// back to the max per-message MODSEQ from the fetched headers.
#[test]
fn header_snapshot_state_falls_back_to_per_message_modseq() {
    let selected = ImapSelectedMailbox {
        mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: ImapUidValidity(7),
        uid_next: None,
        highest_modseq: None,
    };
    let header = imap_header_message_record(
        &selected,
        ImapFetchedHeader {
            mailbox_id: selected.mailbox_id.clone(),
            uid: ImapUid(42),
            modseq: Some(ImapModSeq(900)),
            flags: Vec::new(),
            rfc822_size: 512,
            has_attachment: false,
            headers: b"From: Alice <alice@example.test>\r\nSubject: Hi\r\n\r\n".to_vec(),
            updated_at: "2026-06-29T00:00:00Z".to_string(),
        },
    )
    .expect("mapped header");
    let snapshot = ImapMailboxHeaderSnapshot {
        selected,
        headers: vec![header],
    };

    let state =
        imap_mailbox_state_from_header_snapshot(&snapshot, "2026-06-29T00:00:00Z".to_string());

    assert_eq!(state.highest_modseq, Some(ImapModSeq(900)));
}
