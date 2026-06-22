use super::*;

/// Apply an upsert-only sync chunk: messages/mailboxes are added without any
/// snapshot pruning or cursors, mirroring how a streamed full sync delivers
/// each page before the final reconciliation pass.
fn apply_upsert_chunk(
    store: &DatabaseStore,
    account_id: &AccountId,
    messages: Vec<MessageRecord>,
) -> Result<(), StoreError> {
    store.apply_sync_batch(
        account_id,
        &SyncBatch {
            messages,
            ..SyncBatch::default()
        },
    )?;
    Ok(())
}

fn inbox() -> posthaste_domain::MailboxRecord {
    posthaste_domain::MailboxRecord {
        id: MailboxId::from("inbox"),
        name: "Inbox".to_string(),
        role: Some("inbox".to_string()),
        unread_emails: 0,
        total_emails: 0,
    }
}

#[test]
fn reconcile_prunes_local_messages_absent_from_remote_set() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    // Two messages arrive across upsert-only chunks (no pruning yet).
    apply_upsert_chunk(
        &store,
        &account,
        vec![sample_message("message-1", "inbox", Some("mime-1"))],
    )?;
    apply_upsert_chunk(
        &store,
        &account,
        vec![sample_message("message-2", "inbox", Some("mime-2"))],
    )?;

    // The complete remote set only contains message-2: message-1 was deleted
    // remotely and must be pruned by the final pass.
    store.reconcile_sync(
        &account,
        &SyncReconciliation {
            remote_message_ids: vec![MessageId::from("message-2")],
            remote_mailbox_ids: Vec::new(),
            prune_messages: true,
            prune_mailboxes: false,
            cursors: vec![message_cursor("state-final", "2026-03-31T10:05:00Z")],
        },
    )?;

    let messages = store.list_messages(&account, None)?;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, MessageId::from("message-2"));
    assert!(store
        .get_message_detail(&account, &MessageId::from("message-1"))?
        .is_none());

    // The withheld cursor is committed only by the reconciliation pass.
    let cursor = store.get_cursor(&account, SyncObject::Message)?;
    assert_eq!(
        cursor.map(|cursor| cursor.state),
        Some("state-final".into())
    );
    Ok(())
}

#[test]
fn reconcile_retains_message_delivered_in_a_later_chunk() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    // message-1 arrives in chunk 1, message-2 in chunk 2. A message absent from
    // an earlier chunk but present in a later one must survive reconciliation,
    // because pruning is against the *complete* remote set.
    apply_upsert_chunk(
        &store,
        &account,
        vec![sample_message("message-1", "inbox", Some("mime-1"))],
    )?;
    apply_upsert_chunk(
        &store,
        &account,
        vec![sample_message("message-2", "inbox", Some("mime-2"))],
    )?;

    store.reconcile_sync(
        &account,
        &SyncReconciliation {
            remote_message_ids: vec![MessageId::from("message-1"), MessageId::from("message-2")],
            remote_mailbox_ids: Vec::new(),
            prune_messages: true,
            prune_mailboxes: false,
            cursors: vec![message_cursor("state-final", "2026-03-31T10:05:00Z")],
        },
    )?;

    let messages = store.list_messages(&account, None)?;
    assert_eq!(messages.len(), 2);
    Ok(())
}

#[test]
fn reconcile_prunes_mailboxes_absent_from_remote_set() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    let archive = posthaste_domain::MailboxRecord {
        id: MailboxId::from("archive"),
        name: "Archive".to_string(),
        role: None,
        unread_emails: 0,
        total_emails: 0,
    };
    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: vec![inbox(), archive],
            ..SyncBatch::default()
        },
    )?;

    // Remote now only has the inbox; the archive must be pruned.
    store.reconcile_sync(
        &account,
        &SyncReconciliation {
            remote_message_ids: Vec::new(),
            remote_mailbox_ids: vec![MailboxId::from("inbox")],
            prune_messages: false,
            prune_mailboxes: true,
            cursors: Vec::new(),
        },
    )?;

    let mailboxes = store.list_mailboxes(&account)?;
    assert_eq!(mailboxes.len(), 1);
    assert_eq!(mailboxes[0].id, MailboxId::from("inbox"));
    Ok(())
}

#[test]
fn reconcile_without_prune_flags_only_commits_cursors() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    setup_source(&store, &account, "Primary")?;

    apply_upsert_chunk(
        &store,
        &account,
        vec![sample_message("message-1", "inbox", Some("mime-1"))],
    )?;

    // remote_message_ids is empty but prune_messages is false: nothing is pruned
    // (this is how an incremental sync that streams would behave if reconciled).
    store.reconcile_sync(
        &account,
        &SyncReconciliation {
            remote_message_ids: Vec::new(),
            remote_mailbox_ids: Vec::new(),
            prune_messages: false,
            prune_mailboxes: false,
            cursors: vec![message_cursor("state-final", "2026-03-31T10:05:00Z")],
        },
    )?;

    assert_eq!(store.list_messages(&account, None)?.len(), 1);
    let cursor = store.get_cursor(&account, SyncObject::Message)?;
    assert_eq!(
        cursor.map(|cursor| cursor.state),
        Some("state-final".into())
    );
    Ok(())
}
