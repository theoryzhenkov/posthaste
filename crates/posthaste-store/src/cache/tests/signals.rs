use super::*;

#[test]
fn cache_signal_updates_queue_rescore_candidates_with_search_signals() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    insert_message_metadata(&store, "message-1", "2026-04-27T00:00:00Z")?;
    store.upsert_cache_candidates(&[candidate("message-1", 0.5, 4096)])?;

    store.record_cache_signal_updates(&[CacheSignalUpdate {
        account_id: "primary".to_string(),
        message_id: "message-1".to_string(),
        reason: "search-visible".to_string(),
        search: Some(CacheSearchSignals {
            total_messages: 100,
            result_count: 3,
            result_rank: 1,
        }),
        thread_activity: Some(2.0),
        sender_affinity: Some(1.0),
        local_behavior: None,
        direct_user_boost: Some(0.4),
        pinned: Some(true),
    }])?;

    let candidates = store.list_cache_rescore_candidates(&account, 10)?;

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].message_id, "message-1");
    assert_eq!(candidates[0].search.as_ref().unwrap().result_rank, 1);
    assert!(candidates[0].in_inbox);
    assert!(candidates[0].unread);
    assert!(candidates[0].flagged);
    assert_eq!(candidates[0].thread_activity, 2.0);
    assert_eq!(candidates[0].sender_affinity, 1.0);
    assert_eq!(candidates[0].direct_user_boost, 0.4);
    assert!(candidates[0].pinned);
    assert_eq!(candidates[0].signal_reason, "search-visible");
    assert!(candidates[0].rescore_priority > 100.0);
    Ok(())
}

#[test]
fn rescore_queue_prioritizes_local_signals_over_structural_backlog() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    insert_message_metadata(&store, "old-structural", "2026-04-20T00:00:00Z")?;
    insert_message_metadata(&store, "visible", "2026-04-27T00:00:00Z")?;
    store.write_transaction(|tx| {
        ensure_body_cache_object_tx(
            tx,
            &account,
            &MessageId::from("old-structural"),
            false,
            "body-structural",
            BACKGROUND_RESCORE_PRIORITY,
        )
    })?;

    store.record_cache_signal_updates(&[CacheSignalUpdate {
        account_id: "primary".to_string(),
        message_id: "visible".to_string(),
        reason: "search-visible".to_string(),
        search: Some(CacheSearchSignals {
            total_messages: 100,
            result_count: 3,
            result_rank: 0,
        }),
        thread_activity: None,
        sender_affinity: None,
        local_behavior: None,
        direct_user_boost: Some(0.8),
        pinned: None,
    }])?;

    let candidates = store.list_cache_rescore_candidates(&account, 1)?;

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].message_id, "visible");
    assert_eq!(candidates[0].signal_reason, "search-visible");
    Ok(())
}

#[test]
fn lower_priority_enqueue_does_not_demote_existing_signal_work() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    let message_id = MessageId::from("message-1");
    insert_message_metadata(&store, message_id.as_str(), "2026-04-27T00:00:00Z")?;
    store.record_cache_signal_updates(&[CacheSignalUpdate {
        account_id: "primary".to_string(),
        message_id: message_id.to_string(),
        reason: "search-visible".to_string(),
        search: None,
        thread_activity: None,
        sender_affinity: None,
        local_behavior: None,
        direct_user_boost: Some(0.8),
        pinned: None,
    }])?;
    store.write_transaction(|tx| {
        ensure_body_cache_object_tx(
            tx,
            &account,
            &message_id,
            false,
            "body-structural",
            BACKGROUND_RESCORE_PRIORITY,
        )
    })?;

    let candidates = store.list_cache_rescore_candidates(&account, 1)?;

    assert_eq!(candidates[0].message_id, "message-1");
    assert_eq!(candidates[0].signal_reason, "search-visible");
    assert!(candidates[0].rescore_priority > 100.0);
    Ok(())
}
