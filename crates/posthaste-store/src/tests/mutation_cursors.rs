use super::*;

#[test]
fn raw_message_store_deduplicates_by_hash() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let first = store.store_raw_message(&account, "same mime")?;
    let second = store.store_raw_message(&account, "same mime")?;
    assert_eq!(first.path, second.path);
    assert_eq!(first.sha256, second.sha256);
    Ok(())
}

#[test]
fn set_keywords_persists_cursor_and_none_leaves_existing_state() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![sample_message("message-1", "inbox", Some("mime"))],
        "message-1",
    )?;

    store.set_keywords(
        &account,
        &MessageId::from("message-1"),
        Some(&message_cursor("message-2", "2026-03-31T10:05:00Z")),
        &SetKeywordsCommand {
            add: vec!["$flagged".to_string()],
            remove: Vec::new(),
        },
    )?;
    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)?
            .expect("cursor should exist")
            .state,
        "message-2"
    );

    store.set_keywords(
        &account,
        &MessageId::from("message-1"),
        None,
        &SetKeywordsCommand {
            add: Vec::new(),
            remove: vec!["$flagged".to_string()],
        },
    )?;
    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)?
            .expect("cursor should exist")
            .state,
        "message-2"
    );
    Ok(())
}

#[test]
fn replace_mailboxes_persists_cursor_and_none_leaves_existing_state() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![sample_message("message-1", "inbox", Some("mime"))],
        "message-1",
    )?;

    store.replace_mailboxes(
        &account,
        &MessageId::from("message-1"),
        Some(&message_cursor("message-2", "2026-03-31T10:05:00Z")),
        &ReplaceMailboxesCommand {
            mailbox_ids: vec![MailboxId::from("archive")],
        },
    )?;
    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)?
            .expect("cursor should exist")
            .state,
        "message-2"
    );

    store.replace_mailboxes(
        &account,
        &MessageId::from("message-1"),
        None,
        &ReplaceMailboxesCommand {
            mailbox_ids: vec![MailboxId::from("inbox")],
        },
    )?;
    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)?
            .expect("cursor should exist")
            .state,
        "message-2"
    );
    Ok(())
}

#[test]
fn destroy_message_persists_cursor_and_none_leaves_existing_state() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;
    seed_messages(
        &store,
        &account,
        vec![
            sample_message("message-1", "inbox", Some("mime-1")),
            sample_message("message-2", "inbox", Some("mime-2")),
        ],
        "message-1",
    )?;

    store.destroy_message(
        &account,
        &MessageId::from("message-1"),
        Some(&message_cursor("message-2", "2026-03-31T10:05:00Z")),
    )?;
    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)?
            .expect("cursor should exist")
            .state,
        "message-2"
    );

    store.destroy_message(&account, &MessageId::from("message-2"), None)?;
    assert_eq!(
        store
            .get_cursor(&account, SyncObject::Message)?
            .expect("cursor should exist")
            .state,
        "message-2"
    );
    Ok(())
}
