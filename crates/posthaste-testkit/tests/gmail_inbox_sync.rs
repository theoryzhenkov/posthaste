//! End-to-end Gmail IMAP scenarios against the mock Gmail IMAP server: an
//! initial full-snapshot sync lands the baseline INBOX message in the store and
//! view, and a second sync after a fixture mutation exercises the real
//! CONDSTORE/QRESYNC delta path (`CHANGEDSINCE` + `VANISHED`). The IMAP
//! counterpart to the JMAP live-convergence test, but self-contained (no real
//! server, so it runs unconditionally).
//!
//! Views are queried by the inbox's actual `MailboxId` (resolved via
//! `list_mailboxes`), matching the app: a live-synced Gmail mailbox's id is
//! namespaced (`imap:mailbox:<hex>`), never the bare "inbox" the mock seed path
//! uses, so querying by name returns 0.
//!
// spec: docs/testing/L1#provider-observation-matrix

#[path = "common/mod.rs"]
mod common;

use posthaste_domain_service::AccountId;
use posthaste_client_link::RuntimeLink;
use posthaste_contract_core::{
    AccountScopeRequest, MailListViewState, RuntimeCaller, ViewSnapshot,
};
use posthaste_runtime_api::RuntimeMailReadApi;
use posthaste_testkit::{GmailImapFixture, Harness, RuntimeHarness, SEEDED_SUBJECT};

fn mail_list_rows(snapshot: &ViewSnapshot) -> MailListViewState {
    serde_json::from_value::<MailListViewState>(snapshot.data.clone())
        .expect("snapshot data should be mail list state")
}

/// Open the account's inbox `mailList` view by its real (namespaced) MailboxId,
/// returning the current snapshot's mail-list state.
async fn open_inbox_view(harness: &RuntimeHarness, account: &AccountId) -> MailListViewState {
    let caller = RuntimeCaller::test();
    let mailboxes = harness
        .core()
        .list_mailboxes(
            caller.clone(),
            AccountScopeRequest::Explicit {
                account_ids: vec![account.clone()],
            },
        )
        .await
        .expect("mailboxes should list");
    let inbox = mailboxes
        .get(account)
        .and_then(|ms| {
            ms.iter().find(|m| {
                m.role
                    .as_deref()
                    .is_some_and(|r| r.eq_ignore_ascii_case("inbox"))
                    || m.name.eq_ignore_ascii_case("inbox")
            })
        })
        .expect("inbox mailbox should be present after the initial sync");
    let view = common::mail_list_view(&format!("in:{account}/{}", inbox.id.as_str()));
    let session = harness
        .core()
        .open_session(caller.clone())
        .await
        .expect("session should open")
        .session_id;
    let snapshot = harness
        .core()
        .open_session_view(caller, session, view)
        .await
        .expect("inbox view should open");
    mail_list_rows(&snapshot)
}

/// All row projections joined, for substring assertions over the view contents.
fn row_projections(state: &MailListViewState) -> String {
    state
        .rows
        .iter()
        .map(|r| r.projection.to_string())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[tokio::test]
async fn gmail_imap_sync_lands_inbox_message_in_the_mail_list_view() {
    let gmail = GmailImapFixture::start().await;
    let harness = Harness::new().with_runtime().await;

    // Create + enable (runs discovery) + initial sync (full-snapshot fetch).
    let account = harness.create_gmail_account("gmail-imap", &gmail).await;

    let state = open_inbox_view(&harness, &account).await;
    assert_eq!(
        state.rows.len(),
        1,
        "exactly the one seeded Gmail INBOX message should surface in the view"
    );
    // Prove it is the seeded Gmail message that round-tripped through the IMAP
    // FETCH parse, not an artifact: its projection carries the subject.
    let projection = state.rows[0].projection.to_string();
    assert!(
        projection.contains(SEEDED_SUBJECT),
        "the row projection should carry the seeded subject {SEEDED_SUBJECT:?}, got: {projection}"
    );
    // The per-message authority version (flicker Bug-1b guard input) is stamped
    // from the real IMAP per-message modseq end-to-end: sync -> store ->
    // projection -> view frame. The mock serves the baseline message at
    // modseq 100, so the projection's `version` is 100 — exactly the value the
    // client replica's strict-`<` staleness guard compares.
    assert_eq!(
        state.rows[0]
            .projection
            .get("version")
            .and_then(serde_json::Value::as_u64),
        Some(100),
        "the row projection should carry version=max(modseq); got: {projection}"
    );
}

#[tokio::test]
async fn gmail_imap_qresync_delta_replaces_vanished_message_in_the_view() {
    const NEW_SUBJECT: &str = "Board deck (final)";

    let gmail = GmailImapFixture::start().await;
    let harness = Harness::new().with_runtime().await;

    // Baseline: the seeded message is synced and visible.
    let account = harness
        .create_gmail_account("gmail-imap-delta", &gmail)
        .await;
    let baseline = open_inbox_view(&harness, &account).await;
    assert_eq!(baseline.rows.len(), 1, "baseline should hold one message");
    assert!(
        row_projections(&baseline).contains(SEEDED_SUBJECT),
        "baseline view should show the seeded subject"
    );

    // Mutate the mailbox: expunge the seeded message and deliver a new one,
    // advancing HIGHESTMODSEQ. The next sync must take the QRESYNC delta path
    // (the gateway's STATUS preflight sees a changed MODSEQ, then issues
    // `ENABLE QRESYNC` + `UID FETCH ... (CHANGEDSINCE <modseq> VANISHED)`).
    gmail.vanish_inbox_and_deliver(NEW_SUBJECT);
    harness.sync_account(&account).await;

    // The re-sync must have taken the QRESYNC delta path, not a full snapshot:
    // the mock counts the `CHANGEDSINCE` fetches it answered.
    assert!(
        gmail.changedsince_fetch_count() >= 1,
        "the re-sync should have issued a CHANGEDSINCE (QRESYNC delta) fetch"
    );

    let after = open_inbox_view(&harness, &account).await;
    let projections = row_projections(&after);
    assert_eq!(
        after.rows.len(),
        1,
        "after the delta the view should hold exactly the replacement message, got: {projections}"
    );
    assert!(
        projections.contains(NEW_SUBJECT),
        "the delivered replacement {NEW_SUBJECT:?} should appear after the delta, got: {projections}"
    );
    assert!(
        !projections.contains(SEEDED_SUBJECT),
        "the vanished seeded message should be gone after the delta, got: {projections}"
    );
}
