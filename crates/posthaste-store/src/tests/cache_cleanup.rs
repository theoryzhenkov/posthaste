use super::*;

// spec: docs/L1-sync#cache-object-parity
#[test]
fn deleting_message_removes_cache_object_signals_and_rescore_rows() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("message-1");
    seed_messages(
        &store,
        &account,
        vec![metadata_only_message(message_id.as_str(), "inbox")],
        "state-1",
    )?;
    store.record_cache_signal_updates(&[CacheSignalUpdate {
        account_id: account.to_string(),
        message_id: message_id.to_string(),
        reason: "search-visible".to_string(),
        search: None,
        thread_activity: None,
        sender_affinity: None,
        local_behavior: None,
        direct_user_boost: Some(0.8),
        pinned: None,
    }])?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: Vec::new(),
            messages: Vec::new(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: vec![message_id.clone()],
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    )?;

    assert!(store
        .list_events(&EventFilter {
            account_id: Some(account.clone()),
            topic: Some(EVENT_TOPIC_MESSAGE_UPDATED.to_string()),
            mailbox_id: None,
            after_seq: None,
        })?
        .iter()
        .any(|event| event.message_id.as_ref() == Some(&message_id)
            && event
                .payload
                .get("deleted")
                .and_then(|value| value.as_bool())
                == Some(true)));
    assert_eq!(
        cache_child_count(&store, "cache_object", &account, &message_id)?,
        0
    );
    assert_eq!(
        cache_child_count(&store, "cache_message_signal", &account, &message_id)?,
        0
    );
    assert_eq!(
        cache_child_count(&store, "cache_rescore_queue", &account, &message_id)?,
        0
    );
    Ok(())
}

#[test]
fn deleted_mailbox_removes_memberships_and_imap_state() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("message-1");
    seed_messages(
        &store,
        &account,
        vec![sample_message(message_id.as_str(), "archive", None)],
        "state-1",
    )?;
    let imap_mailbox_id = MailboxId::from("archive");
    let state = ImapMailboxSyncState::new(
        imap_mailbox_id.clone(),
        "Archive".to_string(),
        ImapUidValidity(10),
        "2026-04-25T00:00:00Z".to_string(),
    );
    let location = ImapMessageLocation {
        message_id: message_id.clone(),
        mailbox_id: imap_mailbox_id.clone(),
        uid_validity: ImapUidValidity(10),
        uid: ImapUid(101),
        modseq: None,
        updated_at: "2026-04-25T00:00:00Z".to_string(),
    };
    store.put_imap_mailbox_state(&account, &state)?;
    store.put_imap_message_location(&account, &location)?;

    store.apply_sync_batch(
        &account,
        &SyncBatch {
            mailboxes: Vec::new(),
            messages: Vec::new(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: vec![imap_mailbox_id.clone()],
            deleted_message_ids: Vec::new(),
            replace_all_mailboxes: false,
            replace_all_messages: false,
            cursors: Vec::new(),
        },
    )?;

    assert_eq!(
        store.get_message_mailboxes(&account, &message_id)?,
        Vec::<MailboxId>::new()
    );
    assert!(store
        .get_imap_mailbox_state(&account, &imap_mailbox_id)?
        .is_none());
    assert_eq!(
        store.list_imap_mailbox_message_locations(&account, &imap_mailbox_id)?,
        Vec::<ImapMessageLocation>::new()
    );
    assert!(store
        .list_events(&EventFilter {
            account_id: Some(account),
            topic: Some(EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED.to_string()),
            mailbox_id: None,
            after_seq: None,
        })?
        .iter()
        .any(|event| event.message_id.as_ref() == Some(&message_id)));
    Ok(())
}

// spec: docs/L1-sync#cache-object-parity
#[test]
fn opening_store_repairs_missing_body_cache_objects() -> Result<(), StoreError> {
    let root = temp_root();
    let db_path = root.join("mail.sqlite");
    let data_root = root.join("data");
    let account = AccountId::from("primary");
    let message_id = MessageId::from("legacy-message");
    {
        let store = DatabaseStore::open(&db_path, &data_root)?;
        store.write_transaction(|tx| {
            tx.execute(
                "INSERT INTO message (account_id, id, thread_id, received_at, size)
                     VALUES (?1, ?2, 'thread-1', '2026-04-27T00:00:00Z', 4096)",
                params![account.as_str(), message_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })?;
    }

    let store = DatabaseStore::open(db_path, data_root)?;

    let row = cache_object_row(&store, &account, &message_id)?.expect("cache object");
    assert_eq!(row.0, "wanted");
    assert_eq!(
        cache_child_count(&store, "cache_rescore_queue", &account, &message_id)?,
        1
    );
    Ok(())
}

// spec: docs/L1-sync#cache-object-parity
#[test]
fn opening_store_prunes_orphan_cache_child_rows() -> Result<(), StoreError> {
    let root = temp_root();
    let db_path = root.join("mail.sqlite");
    let data_root = root.join("data");
    let account = AccountId::from("primary");
    let message_id = MessageId::from("orphan-message");
    {
        let _store = DatabaseStore::open(&db_path, &data_root)?;
    }
    {
        let connection =
            Connection::open(&db_path).map_err(|err| StoreError::Failure(err.to_string()))?;
        connection
            .pragma_update(None, "foreign_keys", "OFF")
            .map_err(sql_to_store_error)?;
        connection
                .execute(
                    "INSERT INTO cache_object (
                        account_id, message_id, layer, object_id, fetch_unit, state,
                        value_bytes, fetch_bytes, priority, reason, last_scored_at
                     ) VALUES (?1, ?2, 'body', '', 'body_only', 'wanted', 0, 4096, 1, 'legacy', '2026-04-27T00:00:00Z')",
                    params![account.as_str(), message_id.as_str()],
                )
                .map_err(sql_to_store_error)?;
        connection
            .execute(
                "INSERT INTO cache_message_signal (
                        account_id, message_id, direct_user_boost, dirty_at
                     ) VALUES (?1, ?2, 1, '2026-04-27T00:00:00Z')",
                params![account.as_str(), message_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
        connection
            .execute(
                "INSERT INTO cache_rescore_queue (account_id, message_id, reason, queued_at)
                     VALUES (?1, ?2, 'legacy', '2026-04-27T00:00:00Z')",
                params![account.as_str(), message_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
    }

    let store = DatabaseStore::open(db_path, data_root)?;

    assert_eq!(
        cache_child_count(&store, "cache_object", &account, &message_id)?,
        0
    );
    assert_eq!(
        cache_child_count(&store, "cache_message_signal", &account, &message_id)?,
        0
    );
    assert_eq!(
        cache_child_count(&store, "cache_rescore_queue", &account, &message_id)?,
        0
    );
    Ok(())
}
