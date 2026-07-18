//! End-to-end IMAP move/archive wire-correctness scenarios against the mock
//! Gmail (and generic) IMAP server: a `replace_mailboxes` mutation must
//! actually remove the message from every removed mailbox *on the server*, not
//! just locally. Drives `MailService` + the real `LiveImapSmtpGateway`
//! directly (enqueue → flush → sync).
//!
//! The provider-correctness bug these pin (owner field report, Gmail): the
//! wire delta used to be computed against the local canonical mailbox junction,
//! which the optimistic write-through had already replaced with the
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

use posthaste_domain_model::{AccountId, MailboxId, MessageId, ReplaceMailboxesCommand};
use posthaste_imap::LiveImapSmtpGateway;
use posthaste_testkit::{
    GmailImapFixture, Harness, MAILBOX_ALL_MAIL, MAILBOX_INBOX, MAILBOX_STARRED, MAILBOX_TRASH,
};

use common::{connect_account, flush_settled, mailbox_id_by_name, messages_in, sync};

/// The seeded message's UID on the mock server.
const SEEDED_UID: u32 = 1;

/// Apply a `replace_mailboxes` locally, push it through the real gateway, and
/// assert it settled applied (nothing pending, failed, or parked) — the
/// mutation-Confirmed invariant of the retired runtime drive.
async fn settle_replace(
    harness: &Harness,
    account: &AccountId,
    gateway: &LiveImapSmtpGateway,
    message_id: &MessageId,
    target_mailbox_ids: Vec<MailboxId>,
) {
    harness
        .service
        .replace_mailboxes(
            account,
            message_id,
            &ReplaceMailboxesCommand {
                mailbox_ids: target_mailbox_ids,
            },
        )
        .await
        .expect("mailbox move should apply locally");
    flush_settled(harness, account, gateway).await;
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
            !line.contains("EXPUNGE") || line.contains("UID EXPUNGE"),
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
    let harness = Harness::new();
    let account = AccountId::from("gmail-archive");
    let gateway = connect_account(&harness, &gmail, "gmail-archive").await;

    let inbox = mailbox_id_by_name(&harness, &account, MAILBOX_INBOX);
    let all_mail = mailbox_id_by_name(&harness, &account, MAILBOX_ALL_MAIL);
    let starred = mailbox_id_by_name(&harness, &account, MAILBOX_STARRED);
    let message_id = common::seeded_message_id(&harness, &account, &inbox);

    settle_replace(
        &harness,
        &account,
        &gateway,
        &message_id,
        vec![all_mail.clone(), starred],
    )
    .await;

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
    sync(&harness, &account, &gateway).await;
    sync(&harness, &account, &gateway).await;
    assert_eq!(
        messages_in(&harness, &account, &inbox).len(),
        0,
        "the archived message must not resurrect in the INBOX projection after re-syncs"
    );
    assert_eq!(
        messages_in(&harness, &account, &all_mail).len(),
        1,
        "the archived message should remain in the All Mail projection"
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
    let harness = Harness::new();
    let account = AccountId::from("gmail-move");
    let gateway = connect_account(&harness, &gmail, "gmail-move").await;

    let inbox = mailbox_id_by_name(&harness, &account, MAILBOX_INBOX);
    let all_mail = mailbox_id_by_name(&harness, &account, MAILBOX_ALL_MAIL);
    let starred = mailbox_id_by_name(&harness, &account, MAILBOX_STARRED);
    let trash = mailbox_id_by_name(&harness, &account, MAILBOX_TRASH);
    let message_id = common::seeded_message_id(&harness, &account, &inbox);

    settle_replace(
        &harness,
        &account,
        &gateway,
        &message_id,
        vec![all_mail, starred, trash.clone()],
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

    sync(&harness, &account, &gateway).await;
    assert_eq!(
        messages_in(&harness, &account, &inbox).len(),
        0,
        "INBOX projection should be empty"
    );
    assert_eq!(
        messages_in(&harness, &account, &trash).len(),
        1,
        "Trash projection should hold the message"
    );
}

/// The app-shaped trash flow (`moveToRole("trash")` resolves to
/// `ReplaceMailboxes([trash])`): the delta adds Trash and removes *every*
/// current location. Gmail strips the other labels itself when the message is
/// copied into Trash, so the follow-up removals find the UID already gone —
/// removal is idempotent and the mutation must still settle applied with the
/// message ending up in Trash only.
#[tokio::test]
async fn gmail_trash_flow_tolerates_gmail_stripping_labels_on_copy() {
    let gmail = GmailImapFixture::start_condstore_only().await;
    let harness = Harness::new();
    let account = AccountId::from("gmail-trash");
    let gateway = connect_account(&harness, &gmail, "gmail-trash").await;

    let inbox = mailbox_id_by_name(&harness, &account, MAILBOX_INBOX);
    let trash = mailbox_id_by_name(&harness, &account, MAILBOX_TRASH);
    let message_id = common::seeded_message_id(&harness, &account, &inbox);

    // Target = [Trash] only: add=[Trash], remove=[INBOX, All Mail, Starred] —
    // a non-simple delta (settle_replace asserts settled-applied: the
    // already-gone removals must not fail the mutation).
    settle_replace(
        &harness,
        &account,
        &gateway,
        &message_id,
        vec![trash.clone()],
    )
    .await;

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

    sync(&harness, &account, &gateway).await;
    assert_eq!(
        messages_in(&harness, &account, &inbox).len(),
        0,
        "INBOX projection should be empty"
    );
    assert_eq!(
        messages_in(&harness, &account, &trash).len(),
        1,
        "Trash projection should hold the message"
    );
}

/// Generic IMAP (UIDPLUS, no MOVE, no Gmail label magic): a non-simple move
/// COPYies into the added mailbox and must `UID EXPUNGE` the removed location
/// — a message copied to B but left `\Deleted`-unexpunged in A is still
/// visible in A to other clients.
#[tokio::test]
async fn generic_non_simple_move_expunges_the_removed_location() {
    let imap = GmailImapFixture::start_generic_uidplus().await;
    let harness = Harness::new();
    let account = AccountId::from("generic-move");
    let gateway = connect_account(&harness, &imap, "generic-move").await;

    let inbox = mailbox_id_by_name(&harness, &account, MAILBOX_INBOX);
    let trash = mailbox_id_by_name(&harness, &account, MAILBOX_TRASH);
    let message_id = common::seeded_message_id(&harness, &account, &inbox);

    settle_replace(&harness, &account, &gateway, &message_id, vec![trash]).await;

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
    let harness = Harness::new();
    let account = AccountId::from("generic-basic");
    let gateway = connect_account(&harness, &imap, "generic-basic").await;

    let inbox = mailbox_id_by_name(&harness, &account, MAILBOX_INBOX);
    let trash = mailbox_id_by_name(&harness, &account, MAILBOX_TRASH);
    let message_id = common::seeded_message_id(&harness, &account, &inbox);

    settle_replace(&harness, &account, &gateway, &message_id, vec![trash]).await;

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
