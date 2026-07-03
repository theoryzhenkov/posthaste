//! Snooze store tests: the `message_snooze` CRUD + the "leaving Snoozed clears
//! the row" invariant.
//!
//! @spec docs/eph/DESIGN-L2-snooze

use super::*;

#[test]
fn snooze_insert_list_due_and_delete() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let m1 = MessageId::from("m-1");
    let m2 = MessageId::from("m-2");

    // Insert a due snooze (until = 1000) + a not-yet-due one (until = 2000).
    store.insert_snooze(&account, &m1, 1000)?;
    store.insert_snooze(&account, &m2, 2000)?;

    // list_due_snoozes(now = 1500) → only m-1 (until <= now).
    let due = store.list_due_snoozes(&account, 1500)?;
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].0, m1);
    assert_eq!(due[0].1, 1000);

    // delete_snooze is idempotent (no row → no-op).
    store.delete_snooze(&account, &m1)?;
    store.delete_snooze(&account, &m1)?;
    assert!(store.list_due_snoozes(&account, 1500)?.is_empty());

    // insert_snooze is an upsert (replaces the until on conflict).
    store.insert_snooze(&account, &m2, 500)?;
    let due_earlier = store.list_due_snoozes(&account, 600)?;
    assert_eq!(due_earlier.len(), 1);
    assert_eq!(due_earlier[0].0, m2);
    assert_eq!(due_earlier[0].1, 500);
    Ok(())
}

#[test]
fn replace_mailboxes_clears_the_snooze_row_invariant() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("message-1");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![metadata_only_message(message_id.as_str(), "inbox")],
        "state-1",
    )?;

    // The message is snoozed: insert a snooze row.
    store.insert_snooze(&account, &message_id, 1_000_000)?;
    assert_eq!(store.list_due_snoozes(&account, 2_000_000)?.len(), 1);

    // Any mailbox replace (here: moving it to "archive") clears the snooze row
    // — the invariant. This is what makes undo / a manual move / unsnooze all
    // correct without each path having to remember to delete the row.
    store.replace_mailboxes(
        &account,
        &message_id,
        None,
        &ReplaceMailboxesCommand {
            mailbox_ids: vec![MailboxId::from("archive")],
        },
    )?;
    assert!(
        store.list_due_snoozes(&account, 2_000_000)?.is_empty(),
        "a mailbox replace must clear the snooze row"
    );
    Ok(())
}

// spec: docs/eph/RFC-L2-lifecycle-and-errors#d67 (N15 / M27 sub-unit (b))
#[test]
fn due_snoozes_are_limited_and_drain_across_ticks() -> Result<(), StoreError> {
    // Stand-in for a mass-snooze backlog (N15): more due rows than one
    // `list_due_snoozes` call should ever materialize at once.
    use crate::snooze::SNOOZE_DUE_BATCH_LIMIT;

    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let total = SNOOZE_DUE_BATCH_LIMIT as usize + 25;

    for index in 0..total {
        store.insert_snooze(&account, &MessageId::from(format!("m-{index}")), 1_000)?;
    }

    // First "tick": the store returns at most the batch limit.
    let first_batch = store.list_due_snoozes(&account, 2_000)?;
    assert_eq!(first_batch.len(), SNOOZE_DUE_BATCH_LIMIT as usize);

    // The scheduler tick processes a batch by deleting each returned row
    // (standing in for `clear_snooze_on_mailbox_replace_tx` after the
    // auto-return move) before the next tick's call.
    for (message_id, _until) in &first_batch {
        store.delete_snooze(&account, message_id)?;
    }

    let second_batch = store.list_due_snoozes(&account, 2_000)?;
    assert_eq!(
        second_batch.len(),
        total - SNOOZE_DUE_BATCH_LIMIT as usize,
        "the remainder should surface on the next bounded tick"
    );
    Ok(())
}
