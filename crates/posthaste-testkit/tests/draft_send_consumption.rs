//! End-to-end D125/D126 send-consumes-draft scenarios against the mock Gmail
//! (and generic) IMAP+SMTP fixture: sending a saved draft must destroy the
//! draft on the provider as a settlement effect of the send — the draft leaves
//! Drafts, exactly ONE copy lands in Sent — while a send without a `draft_id`
//! touches nothing.
//!
//! The provider-correctness bug these pin (owner field report): the send
//! request always carried `draft_id`, but nothing consumed it — sending a
//! draft left it sitting in Drafts forever. The fix enqueues the draft's
//! delete when the send settles success (idempotent, retried with the outbox),
//! and the Gmail Sent-copy rider is asserted both ways: Gmail SMTP auto-places
//! the Sent copy (a client APPEND would be the classic duplicate — none may
//! appear on the wire), while a generic provider gets its single Sent copy
//! only from the client's APPEND.
//!
// spec: docs/eph/RFC-L2-drafts#3-decisions-proposed
// spec: docs/testing/L1#provider-observation-matrix

#[path = "common/mod.rs"]
mod common;

use posthaste_domain_model::{AccountId, MessageId, Recipient, SendMessageRequest};
use posthaste_client_link::RuntimeLink;
use posthaste_contract_core::{
    AccountScopeRequest, MailListViewState, RuntimeCaller, ViewSnapshot,
};
use posthaste_runtime_api::{RuntimeMailReadApi, RuntimeMailWriteApi};
use posthaste_testkit::{
    GmailImapFixture, Harness, RuntimeHarness, MAILBOX_DRAFTS, MAILBOX_SENT,
};

/// A compose-shaped request: `draft_id` names the originating draft on a send
/// (None while saving the draft itself — the runtime stamps the stable key).
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

/// Resolve the fixture-served mailbox's runtime `MailboxId` by its IMAP name.
async fn mailbox_id_by_name(harness: &RuntimeHarness, account: &AccountId, name: &str) -> String {
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
    let snapshot: ViewSnapshot = harness
        .core()
        .open_link_view(caller, link, view)
        .await
        .expect("mailbox view should open");
    serde_json::from_value::<MailListViewState>(snapshot.data.clone())
        .expect("snapshot data should be mail list state")
}

/// Save a draft through the runtime and flush it to the provider's Drafts
/// mailbox, returning the stable draft key used.
async fn save_and_flush_draft(
    harness: &RuntimeHarness,
    account: &AccountId,
    fixture: &GmailImapFixture,
    subject: &str,
) -> MessageId {
    let draft_key = MessageId::from("compose-session-1");
    harness
        .core()
        .save_draft(
            RuntimeCaller::test(),
            account.clone(),
            Some(draft_key.clone()),
            compose_request(subject, None),
        )
        .await
        .expect("draft should save");
    harness.sync_account(account).await;
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
    let harness = Harness::new().with_runtime().await;
    let account = harness.create_gmail_account("gmail-send-draft", &gmail).await;

    let draft_key = save_and_flush_draft(&harness, &account, &gmail, "Quarterly reply").await;

    harness
        .core()
        .send_message(
            RuntimeCaller::test(),
            account.clone(),
            compose_request("Quarterly reply", Some(draft_key.as_str())),
        )
        .await
        .expect("send should queue");
    harness.sync_account(&account).await;

    // The submission happened exactly once, and the settled send consumed the
    // draft on the server.
    assert_eq!(gmail.smtp_submission_count(), 1, "exactly one SMTP submission");
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

    // Local store coherence: the sync that settled the send already pulled the
    // deletion — the Drafts view no longer shows the consumed draft.
    let drafts_mailbox = mailbox_id_by_name(&harness, &account, MAILBOX_DRAFTS).await;
    let drafts_view = open_mailbox_view(&harness, &account, &drafts_mailbox).await;
    assert_eq!(
        drafts_view.rows.len(),
        0,
        "the consumed draft must disappear from the local Drafts view"
    );
}

/// Generic IMAP (UIDPLUS, no Gmail auto-place): the draft is likewise consumed
/// on send, and the single Sent copy comes from the client's APPEND — SMTP
/// alone creates no Sent copy on a plain provider, so skipping the APPEND here
/// would be the opposite breakage of the Gmail duplicate.
#[tokio::test]
async fn generic_send_consumes_the_draft_and_appends_the_single_sent_copy() {
    let imap = GmailImapFixture::start_generic_uidplus().await;
    let harness = Harness::new().with_runtime().await;
    let account = harness
        .create_gmail_account("generic-send-draft", &imap)
        .await;

    let draft_key = save_and_flush_draft(&harness, &account, &imap, "Weekly update").await;

    harness
        .core()
        .send_message(
            RuntimeCaller::test(),
            account.clone(),
            compose_request("Weekly update", Some(draft_key.as_str())),
        )
        .await
        .expect("send should queue");
    harness.sync_account(&account).await;

    assert_eq!(imap.smtp_submission_count(), 1, "exactly one SMTP submission");
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
    let drafts_mailbox = mailbox_id_by_name(&harness, &account, MAILBOX_DRAFTS).await;
    let drafts_view = open_mailbox_view(&harness, &account, &drafts_mailbox).await;
    assert_eq!(
        drafts_view.rows.len(),
        0,
        "the consumed draft must disappear from the local Drafts view"
    );
}

/// A send WITHOUT `draft_id` leaves an unrelated saved draft untouched —
/// D126 is strictly opt-in via the carried draft identity.
#[tokio::test]
async fn send_without_draft_id_leaves_saved_drafts_untouched() {
    let gmail = GmailImapFixture::start_condstore_only().await;
    let harness = Harness::new().with_runtime().await;
    let account = harness
        .create_gmail_account("gmail-send-plain", &gmail)
        .await;

    save_and_flush_draft(&harness, &account, &gmail, "Unrelated draft").await;

    harness
        .core()
        .send_message(
            RuntimeCaller::test(),
            account.clone(),
            compose_request("Standalone send", None),
        )
        .await
        .expect("send should queue");
    harness.sync_account(&account).await;

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
