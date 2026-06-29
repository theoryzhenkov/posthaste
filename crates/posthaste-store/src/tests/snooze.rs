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
