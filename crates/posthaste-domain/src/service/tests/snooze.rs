//! Snooze scheduler: `MailService::auto_return_snoozed_messages` returns due
//! snoozed messages to the Inbox. The store invariant (clearing the snooze row
//! on a mailbox replace) is a `DatabaseStore` property covered in
//! `posthaste-store`; here we assert the service orchestration — a due snooze
//! moves to the Inbox, a not-yet-due one is left alone.
//!
//! @spec docs/eph/DESIGN-L2-snooze

use super::*;

#[tokio::test]
async fn auto_return_moves_due_snoozed_messages_to_inbox() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("m-1", &["snoozed"]));
    // Due: until = 100, now = 200 (100 <= 200).
    store
        .insert_snooze(&account, &MessageId::from("m-1"), 100)
        .expect("insert snooze");
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));

    let count = service
        .auto_return_snoozed_messages(&account, 200)
        .await
        .expect("auto-return");

    assert_eq!(count, 1, "one due snooze was returned");
    assert_eq!(
        store
            .get_message_mailboxes(&account, &MessageId::from("m-1"))
            .expect("projection mailbox lookup"),
        vec![MailboxId::from("inbox")],
        "the due snoozed message is moved to the inbox",
    );
}

#[tokio::test]
async fn auto_return_leaves_not_yet_due_snoozes_alone() {
    let account = AccountId::from("primary");
    let store = Arc::new(TestStore::with_message_state("m-1", &["snoozed"]));
    // Not yet due: until = 300, now = 200 (300 > 200).
    store
        .insert_snooze(&account, &MessageId::from("m-1"), 300)
        .expect("insert snooze");
    let service = MailService::new(store.clone(), Arc::new(TestConfig::default()));

    let count = service
        .auto_return_snoozed_messages(&account, 200)
        .await
        .expect("auto-return");

    assert_eq!(count, 0, "a not-yet-due snooze is left alone");
    assert_eq!(
        store
            .get_message_mailboxes(&account, &MessageId::from("m-1"))
            .expect("projection mailbox lookup"),
        vec![MailboxId::from("snoozed")],
        "the not-yet-due message stays in the snoozed mailbox",
    );
}
