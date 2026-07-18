//! End-to-end D125/D126 send-consumes-draft scenarios against the mock Gmail
//! (and generic) IMAP+SMTP fixture: sending a saved draft must destroy the
//! draft on the provider as a settlement effect of the send — the draft leaves
//! Drafts, exactly ONE copy lands in Sent — while a send without a `draft_id`
//! touches nothing. Drives `MailService` + the real `LiveImapSmtpGateway`
//! directly (enqueue → flush → sync).
//!
//! The provider-correctness bug these pin (owner field report): the send
//! request always carried `draft_id`, but nothing consumed it — sending a
//! draft left it sitting in Drafts forever. The fix consumes the draft when
//! the send settles success (idempotent, retried with the outbox), and the
//! Gmail Sent-copy rider is asserted both ways: Gmail SMTP auto-places
//! the Sent copy (a client APPEND would be the classic duplicate — none may
//! appear on the wire), while a generic provider gets its single Sent copy
//! only from the client's APPEND.
//!
// spec: docs/eph/RFC-L2-drafts#3-decisions-proposed
// spec: docs/testing/L1#provider-observation-matrix

#[path = "common/mod.rs"]
mod common;

use posthaste_domain_model::{AccountId, MessageId, OperationKind, Recipient, SendMessageRequest};
use posthaste_imap::LiveImapSmtpGateway;
use posthaste_testkit::{GmailImapFixture, Harness, MAILBOX_DRAFTS, MAILBOX_SENT};

use common::{connect_account, flush_settled, mailbox_id_by_name, messages_in, sync};

/// A compose-shaped request: `draft_id` names the originating draft on a send
/// (None while saving the draft itself — the service stamps the stable key).
fn compose_request(subject: &str, draft_id: Option<&str>) -> SendMessageRequest {
    SendMessageRequest {
        from: Some(Recipient {
            name: Some("Gmail Dev".to_string()),
            email: "dev@gmail.example".to_string(),
        }),
        to: vec![Recipient {
            name: None,
            email: "bob@example.test".to_string(),
        }],
        subject: subject.to_string(),
        body: "The final body, as composed.".to_string(),
        draft_id: draft_id.map(str::to_string),
        ..SendMessageRequest::default()
    }
}

/// Save a draft through the service and flush it to the provider's Drafts
/// mailbox, returning the stable draft key used.
async fn save_and_flush_draft(
    harness: &Harness,
    account: &AccountId,
    gateway: &LiveImapSmtpGateway,
    fixture: &GmailImapFixture,
    subject: &str,
) -> MessageId {
    let draft_key = MessageId::from("compose-session-1");
    harness
        .service
        .save_draft(
            account,
            Some(draft_key.clone()),
            compose_request(subject, None),
        )
        .await
        .expect("draft should save");
    flush_settled(harness, account, gateway).await;
    sync(harness, account, gateway).await;
    assert_eq!(
        fixture.mailbox_message_count(MAILBOX_DRAFTS),
        1,
        "the saved draft must land in the provider Drafts mailbox"
    );
    draft_key
}

/// Every wire command line that APPENDs into `mailbox`.
fn append_commands_into(commands: &[String], mailbox: &str) -> Vec<String> {
    commands
        .iter()
        .filter(|line| line.contains("APPEND") && line.contains(mailbox))
        .cloned()
        .collect()
}

/// Gmail: sending a saved draft consumes it — the draft is expunged from
/// Drafts (UID-scoped, per the shared archive-fix helper) — and the Sent copy
/// exists exactly once, auto-placed by Gmail SMTP. The client must NOT also
/// APPEND to Sent (the classic duplicate the per-provider gate prevents).
#[tokio::test]
async fn gmail_send_consumes_the_draft_with_exactly_one_sent_copy() {
    let gmail = GmailImapFixture::start_condstore_only().await;
    let harness = Harness::new();
    let account = AccountId::from("gmail-send-draft");
    let gateway = connect_account(&harness, &gmail, "gmail-send-draft").await;

    let draft_key =
        save_and_flush_draft(&harness, &account, &gateway, &gmail, "Quarterly reply").await;

    harness
        .service
        .enqueue_send(
            &account,
            compose_request("Quarterly reply", Some(draft_key.as_str())),
        )
        .await
        .expect("send should queue");
    flush_settled(&harness, &account, &gateway).await;
    sync(&harness, &account, &gateway).await;

    // The submission happened exactly once, and the settled send consumed the
    // draft on the server.
    assert_eq!(
        gmail.smtp_submission_count(),
        1,
        "exactly one SMTP submission"
    );
    assert_eq!(
        gmail.mailbox_message_count(MAILBOX_DRAFTS),
        0,
        "the sent draft must be consumed from the provider Drafts mailbox"
    );

    // Exactly ONE Sent copy: the Gmail-SMTP auto-placed one.
    assert_eq!(
        gmail.mailbox_subjects(MAILBOX_SENT),
        vec!["Quarterly reply".to_string()],
        "exactly one Sent copy, auto-placed by Gmail SMTP"
    );

    // The Sent-copy rider on the wire: no client APPEND into Sent on Gmail.
    let commands = gmail.commands();
    let sent_appends = append_commands_into(&commands, MAILBOX_SENT);
    assert!(
        sent_appends.is_empty(),
        "Gmail auto-places the Sent copy; a client APPEND is the classic duplicate: {sent_appends:#?}"
    );
    // The destroy is the UID-scoped expunge in Drafts (shared archive-fix
    // helper), never a mailbox-wide expunge.
    assert_eq!(
        commands
            .iter()
            .filter(|line| line.starts_with(&format!("{MAILBOX_DRAFTS}: "))
                && line.contains("UID EXPUNGE"))
            .count(),
        1,
        "the consumed draft is UID EXPUNGEd from Drafts exactly once, wire: {commands:#?}"
    );

    // Local store coherence: the sync that followed the send already pulled the
    // deletion — the Drafts projection no longer shows the consumed draft.
    let drafts_mailbox = mailbox_id_by_name(&harness, &account, MAILBOX_DRAFTS);
    assert_eq!(
        messages_in(&harness, &account, &drafts_mailbox).len(),
        0,
        "the consumed draft must disappear from the local Drafts projection"
    );
}

/// Generic IMAP (UIDPLUS, no Gmail auto-place): the draft is likewise consumed
/// on send, and the single Sent copy comes from the client's APPEND — SMTP
/// alone creates no Sent copy on a plain provider, so skipping the APPEND here
/// would be the opposite breakage of the Gmail duplicate.
#[tokio::test]
async fn generic_send_consumes_the_draft_and_appends_the_single_sent_copy() {
    let imap = GmailImapFixture::start_generic_uidplus().await;
    let harness = Harness::new();
    let account = AccountId::from("generic-send-draft");
    let gateway = connect_account(&harness, &imap, "generic-send-draft").await;

    let draft_key =
        save_and_flush_draft(&harness, &account, &gateway, &imap, "Weekly update").await;

    harness
        .service
        .enqueue_send(
            &account,
            compose_request("Weekly update", Some(draft_key.as_str())),
        )
        .await
        .expect("send should queue");
    flush_settled(&harness, &account, &gateway).await;
    sync(&harness, &account, &gateway).await;

    assert_eq!(
        imap.smtp_submission_count(),
        1,
        "exactly one SMTP submission"
    );
    assert_eq!(
        imap.mailbox_message_count(MAILBOX_DRAFTS),
        0,
        "the sent draft must be consumed from the provider Drafts mailbox"
    );

    // Exactly ONE Sent copy — created by the client's APPEND (plain SMTP does
    // not place one).
    assert_eq!(
        imap.mailbox_subjects(MAILBOX_SENT),
        vec!["Weekly update".to_string()],
        "exactly one Sent copy, APPENDed by the client"
    );
    let commands = imap.commands();
    assert_eq!(
        append_commands_into(&commands, MAILBOX_SENT).len(),
        1,
        "a generic provider gets its Sent copy from exactly one client APPEND, wire: {commands:#?}"
    );
    assert_eq!(
        commands
            .iter()
            .filter(|line| line.starts_with(&format!("{MAILBOX_DRAFTS}: "))
                && line.contains("UID EXPUNGE"))
            .count(),
        1,
        "the consumed draft is UID EXPUNGEd from Drafts exactly once, wire: {commands:#?}"
    );

    // Local store coherence on the generic shape too.
    let drafts_mailbox = mailbox_id_by_name(&harness, &account, MAILBOX_DRAFTS);
    assert_eq!(
        messages_in(&harness, &account, &drafts_mailbox).len(),
        0,
        "the consumed draft must disappear from the local Drafts projection"
    );
}

/// A send WITHOUT `draft_id` leaves an unrelated saved draft untouched —
/// D126 is strictly opt-in via the carried draft identity.
#[tokio::test]
async fn send_without_draft_id_leaves_saved_drafts_untouched() {
    let gmail = GmailImapFixture::start_condstore_only().await;
    let harness = Harness::new();
    let account = AccountId::from("gmail-send-plain");
    let gateway = connect_account(&harness, &gmail, "gmail-send-plain").await;

    save_and_flush_draft(&harness, &account, &gateway, &gmail, "Unrelated draft").await;

    harness
        .service
        .enqueue_send(&account, compose_request("Standalone send", None))
        .await
        .expect("send should queue");
    flush_settled(&harness, &account, &gateway).await;
    sync(&harness, &account, &gateway).await;

    assert_eq!(gmail.smtp_submission_count(), 1, "the send still submits");
    assert_eq!(
        gmail.mailbox_message_count(MAILBOX_DRAFTS),
        1,
        "a send without draft_id must not touch any draft"
    );
    assert_eq!(
        gmail.mailbox_subjects(MAILBOX_SENT),
        vec!["Standalone send".to_string()],
        "the send still lands its single Sent copy"
    );
}

/// The canonical-id twin fix (D128): a Gmail draft, whose UID the server
/// re-canonicalizes to an `X-GM-MSGID`-based id on sync, must be saved under —
/// and reconciled to — the SAME id sync will observe. Otherwise the save returns
/// a UID-based id that never materializes (an orphaned edit session) while the
/// synced `X-GM-MSGID` row surfaces as a duplicate "twin" in the Drafts list.
///
/// This pins both halves: after save + sync there is exactly ONE Drafts row,
/// under the Gmail canonical id, and a resumed edit's pending `DraftUpdate`
/// carries the stable compose key (resolved to that SAME live id at flush) —
/// proving the compose session was reconciled to the materialized draft, not
/// stranded beside it.
#[tokio::test]
async fn gmail_draft_save_reconciles_to_the_synced_canonical_id_with_no_twin() {
    let gmail = GmailImapFixture::start_condstore_only().await;
    let harness = Harness::new();
    let account = AccountId::from("gmail-draft-twin");
    let gateway = connect_account(&harness, &gmail, "gmail-draft-twin").await;

    // Save + flush + sync (asserts one provider Drafts message internally).
    let draft_key = save_and_flush_draft(&harness, &account, &gateway, &gmail, "Draft v1").await;

    // Exactly one local Drafts row — no transient twin — under the Gmail
    // canonical id the sync materialized.
    let drafts_mailbox = mailbox_id_by_name(&harness, &account, MAILBOX_DRAFTS);
    let drafts_rows = messages_in(&harness, &account, &drafts_mailbox);
    assert_eq!(
        drafts_rows.len(),
        1,
        "exactly one Drafts row after save + sync (no canonical-id twin): {drafts_rows:#?}"
    );
    let row_id = drafts_rows[0].id.to_string();
    assert!(
        row_id.starts_with("imap:gmail:msgid:"),
        "the synced draft row is under the Gmail canonical id, got {row_id}"
    );

    // Resume the edit. The pending DraftUpdate carries the STABLE compose key
    // (M70/D136); the canonical id the sync materialized is resolved at FLUSH —
    // so the no-twin invariant is proven at the provider: flushing the edit
    // must replace the materialized draft in place, not strand the compose
    // session beside an orphaned UID-based twin.
    harness
        .service
        .save_draft(
            &account,
            Some(draft_key.clone()),
            compose_request("Draft v2", None),
        )
        .await
        .expect("resumed edit should save");
    let pending = harness
        .service
        .list_pending_operations(&account)
        .expect("pending operations should list");
    let edit = pending
        .iter()
        .find(|op| op.kind == OperationKind::DraftUpdate)
        .expect("the resumed edit enqueues a DraftUpdate");
    assert_eq!(
        edit.entity.id,
        draft_key.as_str(),
        "the resumed edit carries the stable draft key; the live id is resolved at flush (M70)"
    );

    // Flush the edit: the registry resolves the key to the synced canonical id,
    // so the provider replace lands in place — still exactly ONE provider draft
    // and ONE local Drafts row (no twin).
    flush_settled(&harness, &account, &gateway).await;
    sync(&harness, &account, &gateway).await;
    assert_eq!(
        gmail.mailbox_message_count(MAILBOX_DRAFTS),
        1,
        "the flushed edit replaced the synced draft in place — no provider twin"
    );
    let drafts_after_edit = messages_in(&harness, &account, &drafts_mailbox);
    assert_eq!(
        drafts_after_edit.len(),
        1,
        "exactly one local Drafts row after the resumed edit flushed: {drafts_after_edit:#?}"
    );
}
