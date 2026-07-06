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

#[tokio::test]
async fn destroy_empty_mailbox_calls_the_gateway_then_resyncs() {
    let store = Arc::new(TestStore::default());
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    // Empty `rule_page` => the TestStore reports every mailbox with total 0, so
    // "inbox" is empty and the destroy needs no confirmation.
    let gateway = MutationGateway::with_sync_batch(1, SyncBatch::default());
    let account = AccountId::from("primary");
    let mailbox = MailboxId::from("inbox");

    let events = service
        .destroy_mailbox(&account, &mailbox, false, &gateway)
        .await
        .expect("destroying an empty mailbox needs no confirmation");

    assert_eq!(
        &*gateway
            .destroy_mailbox_calls
            .lock()
            .expect("calls poisoned"),
        &[(mailbox.clone(), false)],
        "the gateway destroy runs exactly once, with remove_emails=false",
    );
    let removed = events
        .iter()
        .find(|event| event.topic == EVENT_TOPIC_MAILBOX_UPDATED)
        .expect("a mailbox-updated event marks the removal");
    assert_eq!(removed.payload["mailboxId"], "inbox");
    assert_eq!(removed.payload["deleted"], true);
}

#[tokio::test]
async fn destroy_non_empty_without_remove_emails_refuses_and_never_calls_the_gateway() {
    // THE SAFETY GATE, un-bypassable from any caller: a non-empty mailbox destroy
    // without the confirmed remove-emails flag returns `MailboxNotEmpty` with the
    // count BEFORE the gateway is ever touched.
    let store = Arc::new(TestStore::default());
    store.rule_page.lock().expect("rule page poisoned").push({
        let mut summary = sample_message_summary("m-archive", Vec::new());
        summary.mailbox_ids = vec![MailboxId::from("archive")];
        summary
    });
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_sync_batch(1, SyncBatch::default());
    let account = AccountId::from("primary");
    let mailbox = MailboxId::from("archive");

    let error = service
        .destroy_mailbox(&account, &mailbox, false, &gateway)
        .await
        .expect_err("a non-empty mailbox must be refused without remove_emails");

    assert!(
        matches!(
            error,
            posthaste_domain_model::ServiceError::Gateway(GatewayError::MailboxNotEmpty {
                count: 1
            })
        ),
        "the refusal carries the message count, got {error:?}",
    );
    assert!(
        gateway
            .destroy_mailbox_calls
            .lock()
            .expect("calls poisoned")
            .is_empty(),
        "the gateway must NOT be called when the gate refuses",
    );
}

#[tokio::test]
async fn destroy_non_empty_with_remove_emails_calls_the_gateway() {
    // With the confirmed flag the same non-empty mailbox proceeds: the gateway
    // destroy runs (with remove_emails=true) and the resync tears the rows down.
    let store = Arc::new(TestStore::default());
    store.rule_page.lock().expect("rule page poisoned").push({
        let mut summary = sample_message_summary("m-archive", Vec::new());
        summary.mailbox_ids = vec![MailboxId::from("archive")];
        summary
    });
    let service = MailService::new(store, Arc::new(TestConfig::default()));
    let gateway = MutationGateway::with_sync_batch(1, SyncBatch::default());
    let account = AccountId::from("primary");
    let mailbox = MailboxId::from("archive");

    service
        .destroy_mailbox(&account, &mailbox, true, &gateway)
        .await
        .expect("a confirmed remove_emails destroy succeeds");

    assert_eq!(
        &*gateway
            .destroy_mailbox_calls
            .lock()
            .expect("calls poisoned"),
        &[(mailbox, true)],
        "the confirmed flag threads through to the gateway",
    );
}
