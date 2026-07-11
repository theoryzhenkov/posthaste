//! Snooze scheduler: `MailService::auto_return_snoozed_messages` returns due
//! snoozed messages to the Inbox via the same `replace_mailboxes` mutation path
//! the client uses — under NS1 the move folds into the OVERLAY plane (base is
//! sync-owned) and the snooze row clears immediately. Here we assert the
//! service orchestration — a due snooze folds to Inbox membership, a
//! not-yet-due one is left alone.
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
    // NS1: the auto-return move is an ordinary optimistic mutation — it queues
    // the op and folds Inbox membership into the overlay row. Canonical/base
    // stays untouched until sync (or a settle readback) rewrites it.
    let overlay = store
        .overlay_rows
        .lock()
        .expect("overlay rows lock poisoned");
    let row = overlay
        .get("m-1")
        .expect("the move folds an overlay entry for the message")
        .as_ref()
        .expect("a mailbox replace folds to a row, not a tombstone");
    assert_eq!(
        row.mailbox_ids,
        vec![MailboxId::from("inbox")],
        "the due snoozed message's overlay row holds inbox membership",
    );
    drop(overlay);
    assert!(
        store
            .snoozes
            .lock()
            .expect("snoozes lock poisoned")
            .is_empty(),
        "the mailbox replace cleared the snooze row immediately",
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
