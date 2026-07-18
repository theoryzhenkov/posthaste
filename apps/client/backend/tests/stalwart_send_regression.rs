//! Regression test for the beta-blocking send-lands-in-Drafts bug: a live-JMAP
//! send with NO prior draft (compose autosave removed) whose provider
//! submission silently "succeeded" while the outgoing copy stayed filed in
//! Drafts and the (same-server) recipient never received it.
//!
//! Root cause: `EmailSubmission/set`'s `onSuccessUpdateEmail` map is keyed by
//! **EmailSubmission** id (RFC 8621 §7.5) but the send keyed it by the
//! *Email's* creation id — an unresolvable reference the server silently
//! ignores, so the Drafts→Sent move never applied. With the deterministic
//! RFC5322 Message-ID (D85) stamped on the outgoing copy, the recipient-side
//! ingest of a same-server delivery then deduplicated against that lingering
//! Drafts copy ("Skipping duplicate message" in Stalwart), so the send
//! vanished entirely — silently, with every method response reporting success.
//!
//! This test drives the REAL pipeline (outbox enqueue → flush → live JMAP
//! gateway → Stalwart) and asserts the provider push actually happened and the
//! outgoing record settled into SENT with nothing left in Drafts.

use std::collections::BTreeSet;

use posthaste_domain_model::{
    AccountDriver, AccountId, MessageSummary, Recipient, SendMessageRequest, SyncTrigger,
};
use posthaste_engine::LiveJmapGateway;
use posthaste_testkit::{Harness, StalwartFixture};

const SUBJECT: &str = "send regression self-send";

fn copies(summaries: &[MessageSummary]) -> Vec<&MessageSummary> {
    summaries
        .iter()
        .filter(|m| m.subject.as_deref() == Some(SUBJECT))
        .collect()
}

#[tokio::test]
async fn jmap_send_without_prior_draft_settles_into_sent_and_leaves_drafts_empty() {
    if std::env::var("POSTHASTE_STALWART_INTEGRATION").as_deref() != Ok("1") {
        eprintln!("skipping Stalwart integration; set POSTHASTE_STALWART_INTEGRATION=1");
        return;
    }
    let stalwart = StalwartFixture::start();
    let harness = Harness::new();
    harness.save_account(
        "jmap-stalwart",
        "Stalwart JMAP",
        AccountDriver::Jmap,
        stalwart.jmap_transport(),
    );
    let gateway = LiveJmapGateway::connect(&stalwart.http_url, Some("dev"), &stalwart.password)
        .await
        .expect("JMAP gateway should connect");
    let account = AccountId::from("jmap-stalwart");
    harness
        .service
        .sync_account(&account, SyncTrigger::Manual, &gateway, None)
        .await
        .expect("initial sync");

    // Compose → Send with NO prior draft: exactly the shape the web client's
    // `message.send` mutation enqueues (draft_id: None).
    harness
        .service
        .enqueue_send(
            &account,
            SendMessageRequest {
                from: Some(Recipient {
                    name: Some("Dev Account".to_string()),
                    email: stalwart.email(),
                }),
                to: vec![Recipient {
                    name: Some("Dev Account".to_string()),
                    email: stalwart.email(),
                }],
                cc: Vec::new(),
                bcc: Vec::new(),
                subject: SUBJECT.to_string(),
                body: "sent through the live JMAP gateway".to_string(),
                ..Default::default()
            },
        )
        .await
        .expect("send queues");

    // The provider push: the send must EXECUTE and settle applied — nothing
    // pending, failed, or parked afterwards.
    harness
        .service
        .flush_account(&account, &gateway)
        .await
        .expect("send flushes");
    let pending = harness
        .service
        .list_pending_operations(&account)
        .expect("pending ops list");
    assert!(
        pending.is_empty(),
        "the send op must settle applied, not linger: {:?}",
        pending
            .iter()
            .map(|op| (op.kind, op.state, op.last_error.clone()))
            .collect::<Vec<_>>()
    );

    // Pull the provider state back and assert where the outgoing copy landed.
    // The Sent-move is part of the submission request itself, so one sync is
    // enough for the Sent copy; give same-server inbound delivery a few rounds.
    let mut summaries = Vec::new();
    for _ in 0..10 {
        harness
            .service
            .sync_account(&account, SyncTrigger::Manual, &gateway, None)
            .await
            .expect("post-send sync");
        summaries = harness
            .service
            .list_messages(&account, None)
            .expect("messages list");
        if !copies(&summaries).is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    let mailboxes = harness.service.list_mailboxes(&account).expect("mailboxes");
    let role_of = |mailbox_id: &posthaste_domain_model::MailboxId| -> String {
        mailboxes
            .iter()
            .find(|mb| &mb.id == mailbox_id)
            .map(|mb| mb.role.clone().unwrap_or_else(|| mb.name.clone()))
            .unwrap_or_else(|| mailbox_id.to_string())
    };
    let found = copies(&summaries);
    assert!(
        !found.is_empty(),
        "the sent message must be visible after sync (the submission must actually happen)"
    );
    let mut landed_in_sent = false;
    for message in &found {
        let labels: BTreeSet<String> = message.mailbox_ids.iter().map(&role_of).collect();
        assert!(
            !labels.contains("drafts"),
            "the sent message must NOT be filed in Drafts (was: {labels:?}) — \
             the onSuccessUpdateEmail Drafts→Sent move must apply"
        );
        landed_in_sent |= labels.contains("sent");
    }
    assert!(
        landed_in_sent,
        "the outgoing copy must settle into the Sent mailbox"
    );
}
