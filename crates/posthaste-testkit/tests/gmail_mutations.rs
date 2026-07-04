//! End-to-end IMAP move/archive wire-correctness scenarios against the mock
//! Gmail (and generic) IMAP server: a `message.replaceMailboxes` mutation must
//! actually remove the message from every removed mailbox *on the server*, not
//! just locally.
//!
//! The provider-correctness bug these pin (owner field report, Gmail): the
//! wire delta used to be computed against the local canonical mailbox junction,
//! which the runtime's optimistic write-through had already replaced with the
//! target set by the time the outbox pushed — so an archive produced an empty
//! delta and **zero wire commands** — and even a nonempty remove delta only
//! parked `UID STORE +FLAGS (\Deleted)` without ever expunging, which no other
//! client observes (Gmail's Auto-Expunge is account configuration we don't
//! control). The fix computes the delta against the sync-owned IMAP locations
//! and, under UIDPLUS, follows the `\Deleted` mark with `UID EXPUNGE` — the
//! UID-scoped form that cannot sweep other clients' deleted messages. Plain
//! `EXPUNGE`/`CLOSE` are answered with `BAD` by the mock, so a regression to
//! the RFC 4315 footgun fails loudly.
//!
// spec: docs/testing/L1#provider-observation-matrix

#[path = "common/mod.rs"]
mod common;

use posthaste_domain_model::AccountId;
use posthaste_client_link::RuntimeLink;
use posthaste_contract_core::{
    AccountScopeRequest, MailListViewState, MutationRequest, RuntimeCaller, ViewSnapshot,
};
use posthaste_runtime_api::RuntimeMailReadApi;
use posthaste_testkit::{
    GmailImapFixture, Harness, RuntimeHarness, MAILBOX_ALL_MAIL, MAILBOX_INBOX, MAILBOX_STARRED,
    MAILBOX_TRASH,
};

/// The seeded message's UID on the mock server.
const SEEDED_UID: u32 = 1;

fn mail_list_rows(snapshot: &ViewSnapshot) -> MailListViewState {
    serde_json::from_value::<MailListViewState>(snapshot.data.clone())
        .expect("snapshot data should be mail list state")
}

/// Resolve the fixture-served mailbox's runtime `MailboxId` by its IMAP name.
async fn mailbox_id_by_name(
    harness: &RuntimeHarness,
    account: &AccountId,
    name: &str,
) -> String {
    let mailboxes = harness
        .core()
        .list_mailboxes(
            RuntimeCaller::test(),
            AccountScopeRequest::Explicit {
                account_ids: vec![account.clone()],
            },
        )
        .await
        .expect("mailboxes should list");
    mailboxes
        .get(account)
        .and_then(|ms| ms.iter().find(|m| m.name.eq_ignore_ascii_case(name)))
        .unwrap_or_else(|| panic!("mailbox {name} should be discovered"))
        .id
        .to_string()
}

/// Open a `mailList` view over one mailbox and return its state.
async fn open_mailbox_view(
    harness: &RuntimeHarness,
    account: &AccountId,
    mailbox_id: &str,
) -> MailListViewState {
    let caller = RuntimeCaller::test();
    let view = common::mail_list_view(&format!("in:{account}/{mailbox_id}"));
    let link = harness
        .core()
        .open_link(caller.clone())
        .await
        .expect("link should open")
        .link_id;
    let snapshot = harness
        .core()
        .open_link_view(caller, link, view)
        .await
        .expect("mailbox view should open");
    mail_list_rows(&snapshot)
}

/// The synced message's id, read from the INBOX view's single row projection.
async fn seeded_message_id(
    harness: &RuntimeHarness,
    account: &AccountId,
    inbox_id: &str,
) -> String {
    let state = open_mailbox_view(harness, account, inbox_id).await;
    assert_eq!(state.rows.len(), 1, "the seeded message should be in INBOX");
    state.rows[0]
        .projection
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("row projection should carry the message id")
        .to_string()
}

fn replace_mailboxes_mutation(
    account_id: &str,
    message_id: &str,
    mailbox_ids: &[&str],
    cmid: &str,
) -> MutationRequest {
    serde_json::from_value(serde_json::json!({
        "name": "message.replaceMailboxes",
        "args": {
            "sourceId": account_id,
            "messageId": message_id,
            "mailboxIds": mailbox_ids,
        },
        "clientMutationId": cmid,
    }))
    .expect("request builds from the flat wire shape")
}

/// Run a `replaceMailboxes` mutation through the full runtime (link + outbox +
/// gateway push) and wait for its settlement.
async fn settle_replace(
    harness: &RuntimeHarness,
    account: &AccountId,
    message_id: &str,
    target_mailbox_ids: &[&str],
    view_mailbox_id: &str,
) {
    let settlement = harness
        .settle(
            replace_mailboxes_mutation(account.as_str(), message_id, target_mailbox_ids, "c-1"),
            common::mail_list_view(&format!("in:{account}/{view_mailbox_id}")),
        )
        .await;
    settlement.assert_confirmed();
}

/// Commands the server saw while `mailbox` was selected, matching `needle`.
fn commands_in(commands: &[String], mailbox: &str, needle: &str) -> Vec<usize> {
    commands
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with(&format!("{mailbox}: ")) && line.contains(needle))
        .map(|(index, _)| index)
        .collect()
}

/// No mailbox-wide expunge anywhere on the wire: plain `EXPUNGE` (RFC 4315
/// footgun — removes other clients' `\Deleted` messages too) and `CLOSE` are
/// both forbidden; only the UID-scoped `UID EXPUNGE` may appear.
fn assert_no_mailbox_wide_expunge(commands: &[String]) {
    for line in commands {
        assert!(
            !(line.contains("EXPUNGE") && !line.contains("UID EXPUNGE")),
            "plain EXPUNGE must never be issued, got: {line}"
        );
        assert!(
            !line.contains(" CLOSE"),
            "CLOSE-based expunge must never be issued, got: {line}"
        );
    }
}

/// Archive on Gmail is a remove-only delta: the message already lives in
/// All Mail (label model), so the target set keeps All Mail + Starred and
/// drops only INBOX. The wire must be `UID STORE +FLAGS (\Deleted)` followed
/// by `UID EXPUNGE` **in INBOX** (Gmail: expunge-from-INBOX removes the
/// `\Inbox` label = archive), the message must leave INBOX on the server while
/// staying in All Mail, and the next syncs must not resurrect the INBOX
/// membership.
#[tokio::test]
async fn gmail_archive_expunges_the_removed_inbox_location() {
    let gmail = GmailImapFixture::start_condstore_only().await;
    let harness = Harness::new().with_runtime().await;
    let account = harness.create_gmail_account("gmail-archive", &gmail).await;

    let inbox = mailbox_id_by_name(&harness, &account, MAILBOX_INBOX).await;
    let all_mail = mailbox_id_by_name(&harness, &account, MAILBOX_ALL_MAIL).await;
    let starred = mailbox_id_by_name(&harness, &account, MAILBOX_STARRED).await;
    let message_id = seeded_message_id(&harness, &account, &inbox).await;

    settle_replace(&harness, &account, &message_id, &[&all_mail, &starred], &inbox).await;

    // The wire: mark + UID EXPUNGE in INBOX, in that order, and no
    // mailbox-wide expunge anywhere.
    let commands = gmail.commands();
    let stores = commands_in(&commands, MAILBOX_INBOX, "UID STORE");
    let store = *stores
        .iter()
        .find(|index| commands[**index].contains("\\Deleted"))
        .unwrap_or_else(|| {
            panic!("archive should UID STORE \\Deleted in INBOX, wire: {commands:#?}")
        });
    let expunges = commands_in(&commands, MAILBOX_INBOX, "UID EXPUNGE");
    assert_eq!(
        expunges.len(),
        1,
        "archive should UID EXPUNGE the removed INBOX location exactly once, wire: {commands:#?}"
    );
    assert!(
        store < expunges[0],
        "\\Deleted must be set before the UID EXPUNGE"
    );
    assert_no_mailbox_wide_expunge(&commands);

    // Server state (what other clients see): gone from INBOX, still in
    // All Mail and Starred.
    assert!(
        !gmail.mailbox_contains_uid(MAILBOX_INBOX, SEEDED_UID),
        "the archived message must actually leave INBOX on the server"
    );
    assert!(
        gmail.mailbox_contains_uid(MAILBOX_ALL_MAIL, SEEDED_UID),
        "the archived message must remain in All Mail"
    );
    assert!(
        gmail.mailbox_contains_uid(MAILBOX_STARRED, SEEDED_UID),
        "archiving must not unstar the message"
    );

    // Sync coherence: the next syncs observe the provider state and must not
    // resurrect the INBOX membership.
    harness.sync_account(&account).await;
    harness.sync_account(&account).await;
    let inbox_view = open_mailbox_view(&harness, &account, &inbox).await;
    assert_eq!(
        inbox_view.rows.len(),
        0,
        "the archived message must not resurrect in the INBOX view after re-syncs"
    );
    let all_mail_view = open_mailbox_view(&harness, &account, &all_mail).await;
    assert_eq!(
        all_mail_view.rows.len(),
        1,
        "the archived message should remain in the All Mail view"
    );
}

/// A one-source -> one-target delta on a MOVE-capable server (real Gmail
/// advertises MOVE) takes the `UID MOVE` path: keeping All Mail + Starred and
/// adding Trash while dropping INBOX is add=[Trash] remove=[INBOX]. On Gmail,
/// moving into Trash strips every other label server-side — the fixture
/// models that, and the next sync must converge on trash-only membership.
#[tokio::test]
async fn gmail_simple_move_to_trash_uses_uid_move() {
    let gmail = GmailImapFixture::start_condstore_only().await;
    let harness = Harness::new().with_runtime().await;
    let account = harness.create_gmail_account("gmail-move", &gmail).await;

    let inbox = mailbox_id_by_name(&harness, &account, MAILBOX_INBOX).await;
    let all_mail = mailbox_id_by_name(&harness, &account, MAILBOX_ALL_MAIL).await;
    let starred = mailbox_id_by_name(&harness, &account, MAILBOX_STARRED).await;
    let trash = mailbox_id_by_name(&harness, &account, MAILBOX_TRASH).await;
    let message_id = seeded_message_id(&harness, &account, &inbox).await;

    settle_replace(
        &harness,
        &account,
        &message_id,
        &[&all_mail, &starred, &trash],
        &inbox,
    )
    .await;

    let commands = gmail.commands();
    assert_eq!(
        commands_in(&commands, MAILBOX_INBOX, "UID MOVE").len(),
        1,
        "a 1:1 mailbox delta on a MOVE server should use UID MOVE, wire: {commands:#?}"
    );
    assert_no_mailbox_wide_expunge(&commands);

    assert!(
        !gmail.mailbox_contains_uid(MAILBOX_INBOX, SEEDED_UID),
        "the moved message must leave INBOX on the server"
    );
    assert!(
        gmail.mailbox_contains_uid(MAILBOX_TRASH, SEEDED_UID),
        "the moved message must be in Trash on the server"
    );

    harness.sync_account(&account).await;
    let inbox_view = open_mailbox_view(&harness, &account, &inbox).await;
    assert_eq!(inbox_view.rows.len(), 0, "INBOX view should be empty");
    let trash_view = open_mailbox_view(&harness, &account, &trash).await;
    assert_eq!(trash_view.rows.len(), 1, "Trash view should hold the message");
}

/// The app-shaped trash flow (`moveToRole("trash")` resolves to
/// `ReplaceMailboxes([trash])`): the delta adds Trash and removes *every*
/// current location. Gmail strips the other labels itself when the message is
/// copied into Trash, so the follow-up removals find the UID already gone —
/// removal is idempotent and the mutation must still settle Confirmed with the
/// message ending up in Trash only.
#[tokio::test]
async fn gmail_trash_flow_tolerates_gmail_stripping_labels_on_copy() {
    let gmail = GmailImapFixture::start_condstore_only().await;
    let harness = Harness::new().with_runtime().await;
    let account = harness.create_gmail_account("gmail-trash", &gmail).await;

    let inbox = mailbox_id_by_name(&harness, &account, MAILBOX_INBOX).await;
    let trash = mailbox_id_by_name(&harness, &account, MAILBOX_TRASH).await;
    let message_id = seeded_message_id(&harness, &account, &inbox).await;

    // Target = [Trash] only: add=[Trash], remove=[INBOX, All Mail, Starred] —
    // a non-simple delta (settle_replace asserts Confirmed: the already-gone
    // removals must not fail the mutation).
    settle_replace(&harness, &account, &message_id, &[&trash], &inbox).await;

    let commands = gmail.commands();
    assert_eq!(
        commands_in(&commands, MAILBOX_INBOX, "UID COPY").len(),
        1,
        "the non-simple trash flow should COPY the message into Trash, wire: {commands:#?}"
    );
    assert_no_mailbox_wide_expunge(&commands);

    assert!(
        gmail.mailbox_contains_uid(MAILBOX_TRASH, SEEDED_UID),
        "the trashed message must be in Trash on the server"
    );
    assert!(
        !gmail.mailbox_contains_uid(MAILBOX_INBOX, SEEDED_UID),
        "the trashed message must leave INBOX on the server"
    );
    assert!(
        !gmail.mailbox_contains_uid(MAILBOX_ALL_MAIL, SEEDED_UID),
        "a trashed message is not in All Mail on Gmail"
    );

    harness.sync_account(&account).await;
    let inbox_view = open_mailbox_view(&harness, &account, &inbox).await;
    assert_eq!(inbox_view.rows.len(), 0, "INBOX view should be empty");
    let trash_view = open_mailbox_view(&harness, &account, &trash).await;
    assert_eq!(trash_view.rows.len(), 1, "Trash view should hold the message");
}

/// Generic IMAP (UIDPLUS, no MOVE, no Gmail label magic): a non-simple move
/// COPYies into the added mailbox and must `UID EXPUNGE` the removed location
/// — a message copied to B but left `\Deleted`-unexpunged in A is still
/// visible in A to other clients.
#[tokio::test]
async fn generic_non_simple_move_expunges_the_removed_location() {
    let imap = GmailImapFixture::start_generic_uidplus().await;
    let harness = Harness::new().with_runtime().await;
    let account = harness.create_gmail_account("generic-move", &imap).await;

    let inbox = mailbox_id_by_name(&harness, &account, MAILBOX_INBOX).await;
    let trash = mailbox_id_by_name(&harness, &account, MAILBOX_TRASH).await;
    let message_id = seeded_message_id(&harness, &account, &inbox).await;

    settle_replace(&harness, &account, &message_id, &[&trash], &inbox).await;

    let commands = imap.commands();
    assert_eq!(
        commands_in(&commands, MAILBOX_INBOX, "UID COPY").len(),
        1,
        "a MOVE-less server should COPY into the added mailbox, wire: {commands:#?}"
    );
    let stores = commands_in(&commands, MAILBOX_INBOX, "UID STORE");
    assert!(
        stores
            .iter()
            .any(|index| commands[*index].contains("\\Deleted")),
        "the removed location should be marked \\Deleted, wire: {commands:#?}"
    );
    assert_eq!(
        commands_in(&commands, MAILBOX_INBOX, "UID EXPUNGE").len(),
        1,
        "the removed location must be UID EXPUNGEd under UIDPLUS, wire: {commands:#?}"
    );
    assert_no_mailbox_wide_expunge(&commands);

    assert!(
        !imap.mailbox_contains_uid(MAILBOX_INBOX, SEEDED_UID),
        "the moved message must actually leave the source mailbox on the server"
    );
    assert!(
        imap.mailbox_contains_uid(MAILBOX_TRASH, SEEDED_UID),
        "the moved message must be in the target mailbox on the server"
    );
}

/// Without UIDPLUS there is no UID-scoped expunge, and the mailbox-wide plain
/// `EXPUNGE` (RFC 4315) would remove other clients' `\Deleted` messages too —
/// so the pinned fallback is mark-`\Deleted`-only: the residual stays in the
/// source mailbox (flagged deleted) until the server or another client
/// expunges it, and no EXPUNGE of any kind is issued.
#[tokio::test]
async fn non_uidplus_removal_falls_back_to_mark_deleted_without_expunge() {
    let imap = GmailImapFixture::start_generic_without_uidplus().await;
    let harness = Harness::new().with_runtime().await;
    let account = harness.create_gmail_account("generic-basic", &imap).await;

    let inbox = mailbox_id_by_name(&harness, &account, MAILBOX_INBOX).await;
    let trash = mailbox_id_by_name(&harness, &account, MAILBOX_TRASH).await;
    let message_id = seeded_message_id(&harness, &account, &inbox).await;

    settle_replace(&harness, &account, &message_id, &[&trash], &inbox).await;

    let commands = imap.commands();
    let stores = commands_in(&commands, MAILBOX_INBOX, "UID STORE");
    assert!(
        stores
            .iter()
            .any(|index| commands[*index].contains("\\Deleted")),
        "the removed location should be marked \\Deleted, wire: {commands:#?}"
    );
    assert!(
        commands.iter().all(|line| !line.contains("EXPUNGE")),
        "no expunge of any kind may be issued without UIDPLUS, wire: {commands:#?}"
    );

    // The pinned residual: the message is still in the source mailbox, marked
    // `\Deleted`, awaiting a server-side expunge.
    assert!(
        imap.mailbox_contains_uid(MAILBOX_INBOX, SEEDED_UID),
        "without UIDPLUS the source copy remains (\\Deleted residual)"
    );
    assert!(
        imap.is_marked_deleted_in(MAILBOX_INBOX, SEEDED_UID),
        "the residual must carry the \\Deleted mark"
    );
    assert!(
        imap.mailbox_contains_uid(MAILBOX_TRASH, SEEDED_UID),
        "the copy into the target mailbox still happens"
    );
}
