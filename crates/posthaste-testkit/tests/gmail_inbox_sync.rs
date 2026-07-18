//! End-to-end Gmail IMAP scenarios against the mock Gmail IMAP server: an
//! initial full-snapshot sync lands the baseline INBOX message in the store and
//! projection, and a second sync after a fixture mutation exercises the real
//! CONDSTORE/QRESYNC delta path (`CHANGEDSINCE` + `VANISHED`). The IMAP
//! counterpart to the JMAP live-convergence coverage, but self-contained (no
//! real server, so it runs unconditionally). Drives `MailService` + the real
//! `LiveImapSmtpGateway` directly.
//!
//! Messages are read by the inbox's actual `MailboxId` (resolved via
//! `list_mailboxes`), matching the app: a live-synced Gmail mailbox's id is
//! namespaced (`imap:mailbox:<hex>`), never the bare "inbox" the mock seed path
//! uses.
//!
// spec: docs/testing/L1#provider-observation-matrix

#[path = "common/mod.rs"]
mod common;

use posthaste_domain_model::AccountId;
use posthaste_testkit::{GmailImapFixture, Harness, MAILBOX_INBOX, SEEDED_SUBJECT};

use common::{connect_account, mailbox_id_by_name, messages_in, sync};

/// The subjects currently projected into the inbox, joined for assertions.
fn inbox_subjects(harness: &Harness, account: &AccountId) -> Vec<String> {
    let inbox = mailbox_id_by_name(harness, account, MAILBOX_INBOX);
    messages_in(harness, account, &inbox)
        .into_iter()
        .map(|m| m.subject.unwrap_or_default())
        .collect()
}

#[tokio::test]
async fn gmail_imap_sync_lands_inbox_message_in_the_projection() {
    let gmail = GmailImapFixture::start().await;
    let harness = Harness::new();
    let account = AccountId::from("gmail-imap");

    // Save + connect + initial sync (full-snapshot fetch).
    let _gateway = connect_account(&harness, &gmail, "gmail-imap").await;

    let inbox = mailbox_id_by_name(&harness, &account, MAILBOX_INBOX);
    let rows = messages_in(&harness, &account, &inbox);
    assert_eq!(
        rows.len(),
        1,
        "exactly the one seeded Gmail INBOX message should surface in the projection"
    );
    // Prove it is the seeded Gmail message that round-tripped through the IMAP
    // FETCH parse, not an artifact: its projection carries the subject.
    assert_eq!(
        rows[0].subject.as_deref(),
        Some(SEEDED_SUBJECT),
        "the projection should carry the seeded subject"
    );
    // The per-message authority version (flicker Bug-1b guard input) is stamped
    // from the real IMAP per-message modseq end-to-end: sync -> store ->
    // projection. The mock serves the baseline message at modseq 100, so the
    // summary's `version` is 100 — exactly the value the client replica's
    // strict-`<` staleness guard compares.
    assert_eq!(
        rows[0].version,
        Some(100),
        "the projection should carry version=max(modseq); got: {:?}",
        rows[0]
    );
}

#[tokio::test]
async fn gmail_imap_qresync_delta_replaces_vanished_message_in_the_projection() {
    const NEW_SUBJECT: &str = "Board deck (final)";

    let gmail = GmailImapFixture::start().await;
    let harness = Harness::new();
    let account = AccountId::from("gmail-imap-delta");

    // Baseline: the seeded message is synced and visible.
    let gateway = connect_account(&harness, &gmail, "gmail-imap-delta").await;
    let baseline = inbox_subjects(&harness, &account);
    assert_eq!(baseline.len(), 1, "baseline should hold one message");
    assert_eq!(
        baseline[0], SEEDED_SUBJECT,
        "baseline projection should show the seeded subject"
    );

    let baseline_headers = gmail.header_fetch_count();

    // Mutate the mailbox: expunge the seeded message and deliver a new one,
    // advancing HIGHESTMODSEQ. The next sync must take the QRESYNC delta path
    // (the gateway's STATUS preflight sees a changed MODSEQ, then issues
    // `ENABLE QRESYNC` + `UID FETCH ... (CHANGEDSINCE <modseq> VANISHED)`).
    gmail.vanish_inbox_and_deliver(NEW_SUBJECT);
    sync(&harness, &account, &gateway).await;

    // The re-sync must have taken the QRESYNC delta path, not a full snapshot:
    // the mock counts the `CHANGEDSINCE` fetches it answered.
    assert!(
        gmail.changedsince_fetch_count() >= 1,
        "the re-sync should have issued a CHANGEDSINCE (QRESYNC delta) fetch"
    );
    // Incrementality proof: the delta fetched exactly the one replacement
    // message's header, once per mailbox view it appears in (the label-model
    // mock serves an `\Inbox`-labeled message from INBOX and All Mail) —
    // nothing else was re-fetched.
    assert_eq!(
        gmail.header_fetch_count(),
        baseline_headers + 2,
        "the QRESYNC delta should fetch exactly the replacement message's header (INBOX + All Mail)"
    );

    let after = inbox_subjects(&harness, &account);
    assert_eq!(
        after.len(),
        1,
        "after the delta the projection should hold exactly the replacement message, got: {after:?}"
    );
    assert!(
        after.contains(&NEW_SUBJECT.to_string()),
        "the delivered replacement {NEW_SUBJECT:?} should appear after the delta, got: {after:?}"
    );
    assert!(
        !after.contains(&SEEDED_SUBJECT.to_string()),
        "the vanished seeded message should be gone after the delta, got: {after:?}"
    );
}

/// The M35a zero-refetch gate, against the Gmail-faithful CONDSTORE-only
/// server (real Gmail advertises CONDSTORE but never QRESYNC): a second sync
/// with no mailbox changes must not re-fetch a single header. Before the fix
/// the executor's CondstoreDelta arm ran a full header snapshot, re-fetching
/// every message on every sync.
#[tokio::test]
async fn condstore_only_second_sync_with_no_changes_fetches_zero_headers() {
    let gmail = GmailImapFixture::start_condstore_only().await;
    let harness = Harness::new();
    let account = AccountId::from("gmail-condstore-nochange");

    let gateway = connect_account(&harness, &gmail, "gmail-condstore-nochange").await;
    let baseline_headers = gmail.header_fetch_count();
    assert!(
        baseline_headers >= 1,
        "the initial full-snapshot sync should have fetched the seeded header"
    );

    sync(&harness, &account, &gateway).await;

    assert_eq!(
        gmail.header_fetch_count(),
        baseline_headers,
        "a no-change CONDSTORE-only re-sync must fetch zero headers"
    );
    let after = inbox_subjects(&harness, &account);
    assert_eq!(
        after.len(),
        1,
        "the seeded message should still be projected after the no-op re-sync"
    );
}

/// CONDSTORE-only incrementality: delivering one message into a two-message
/// mailbox must fetch exactly the one new header — the two unchanged messages
/// are not re-fetched (the pre-fix executor re-fetched all three).
#[tokio::test]
async fn condstore_only_delta_fetches_only_the_changed_messages() {
    const SECOND_SUBJECT: &str = "Roadmap review";
    const NEW_SUBJECT: &str = "Standup notes";

    let gmail = GmailImapFixture::start_condstore_only().await;
    // Two messages in INBOX before the account exists, so the initial snapshot
    // lands both and the later delta has unchanged messages to NOT re-fetch.
    gmail.deliver_additional(SECOND_SUBJECT);
    let harness = Harness::new();
    let account = AccountId::from("gmail-condstore-delta");

    let gateway = connect_account(&harness, &gmail, "gmail-condstore-delta").await;
    let baseline_headers = gmail.header_fetch_count();
    // The label-model mock serves each message once per mailbox view it
    // appears in: the seeded message (labels \Inbox + \Starred) from INBOX,
    // All Mail, and [Gmail]/Starred, the second (\Inbox) from INBOX and
    // All Mail.
    assert_eq!(
        baseline_headers, 5,
        "the initial snapshot should have fetched both seeded headers across their mailbox views"
    );

    gmail.deliver_additional(NEW_SUBJECT);
    sync(&harness, &account, &gateway).await;

    assert!(
        gmail.changedsince_fetch_count() >= 1,
        "the re-sync should have issued a CHANGEDSINCE (CONDSTORE delta) fetch"
    );
    assert_eq!(
        gmail.header_fetch_count(),
        baseline_headers + 2,
        "the CONDSTORE delta should fetch exactly the one delivered message's header (INBOX + All Mail)"
    );

    let after = inbox_subjects(&harness, &account);
    assert_eq!(
        after.len(),
        3,
        "all three messages should be projected after the delta, got: {after:?}"
    );
    assert!(
        after.contains(&NEW_SUBJECT.to_string()),
        "the delivered {NEW_SUBJECT:?} should appear after the delta, got: {after:?}"
    );
    assert!(
        after.contains(&SEEDED_SUBJECT.to_string()) && after.contains(&SECOND_SUBJECT.to_string()),
        "the unchanged messages should survive the partial-delta batch, got: {after:?}"
    );
}

/// CONDSTORE-only deletion detection: CHANGEDSINCE cannot report expunges
/// without QRESYNC, so the delta reconciles a header-free `UID SEARCH
/// UNDELETED` against known local locations — the expunged message leaves the
/// projection while zero headers are fetched.
#[tokio::test]
async fn condstore_only_delta_detects_an_expunged_message_as_deleted() {
    let gmail = GmailImapFixture::start_condstore_only().await;
    let harness = Harness::new();
    let account = AccountId::from("gmail-condstore-expunge");

    let gateway = connect_account(&harness, &gmail, "gmail-condstore-expunge").await;
    let baseline = inbox_subjects(&harness, &account);
    assert_eq!(baseline.len(), 1, "baseline should hold one message");
    let baseline_headers = gmail.header_fetch_count();

    gmail.expunge_inbox();
    sync(&harness, &account, &gateway).await;

    assert_eq!(
        gmail.header_fetch_count(),
        baseline_headers,
        "a pure-expunge CONDSTORE-only re-sync must fetch zero headers"
    );
    let after = inbox_subjects(&harness, &account);
    assert_eq!(
        after.len(),
        0,
        "the expunged message should be removed from the projection, got: {after:?}"
    );
}
