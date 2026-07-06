//! `set_mailbox_role`: a Posthaste-local role (no provider equivalent, e.g.
//! `snooze`) is written as a local override + skips the gateway round-trip;
//! a standard/provider role still goes through the gateway.
//!
//! @spec docs/eph/DESIGN-L2-snooze

use super::*;

#[tokio::test]
async fn local_only_role_skips_the_gateway() {
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    // This gateway's `set_mailbox_role` always rejects ("unused"), so an Ok
    // result proves the gateway round-trip was skipped.
    let gateway = MutationGateway::with_revision(1);
    let account = AccountId::from("primary");
    let mailbox = MailboxId::from("mb-junk");

    let events = service
        .set_mailbox_role(&account, &mailbox, Some("snooze"), &gateway)
        .await
        .expect("the snooze role is local-only — the gateway must be skipped");
    assert_eq!(
        events.len(),
        1,
        "the local override emits one mailbox event"
    );
    assert_eq!(events[0].topic, EVENT_TOPIC_MAILBOX_UPDATED);
}

#[tokio::test]
async fn standard_role_still_uses_the_gateway() {
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    // The gateway rejects `set_mailbox_role`, so a standard role (which routes
    // through the gateway) must surface that error rather than succeeding.
    let gateway = MutationGateway::with_revision(1);
    let account = AccountId::from("primary");
    let mailbox = MailboxId::from("mb-x");

    let result = service
        .set_mailbox_role(&account, &mailbox, Some("archive"), &gateway)
        .await;
    assert!(
        result.is_err(),
        "a standard role goes through the gateway (which rejects here)"
    );
}

#[tokio::test]
async fn create_mailbox_creates_then_resyncs() {
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    // `create_mailbox` runs a blocking gateway create then a resync readback.
    // An (empty) sync batch lets that readback complete — in production the
    // batch carries the new mailbox; the mock gateway mints a deterministic id
    // (`mb-<name>`) so we can assert it threaded into the emitted event.
    let gateway = MutationGateway::with_sync_batch(1, SyncBatch::default());
    let account = AccountId::from("primary");

    let events = service
        .create_mailbox(&account, "Receipts", &gateway)
        .await
        .expect("create should succeed");

    let created = events
        .iter()
        .find(|event| event.topic == EVENT_TOPIC_MAILBOX_UPDATED)
        .expect("a mailbox-updated event is emitted for the created mailbox");
    assert_eq!(
        created.payload["mailboxId"], "mb-Receipts",
        "the event carries the id the gateway minted for the new mailbox"
    );
    assert_eq!(
        created.mailbox_id.as_ref().map(MailboxId::as_str),
        Some("mb-Receipts"),
    );
}
