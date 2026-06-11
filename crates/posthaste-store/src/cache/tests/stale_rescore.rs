use super::*;

#[test]
fn stale_rescore_priority_stays_below_local_signal_work() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    insert_message_metadata(&store, "stale-high", "2026-04-20T00:00:00Z")?;
    insert_message_metadata(&store, "visible", "2026-04-27T00:00:00Z")?;
    store.upsert_cache_candidates(&[candidate("stale-high", 500.0, 4096)])?;
    set_last_scored_at(&store, "stale-high", "2026-04-20T00:00:00Z")?;

    let queued =
        store.queue_stale_cache_rescore_candidates(&account, "2026-04-22T00:00:00Z", 10)?;
    assert_eq!(queued, 1);

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

    let candidates = store.list_cache_rescore_candidates(&account, 2)?;

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.message_id.as_str())
            .collect::<Vec<_>>(),
        vec!["visible", "stale-high"]
    );
    assert!(candidates[1].rescore_priority < 100.0);
    Ok(())
}

#[test]
fn opening_store_migrates_existing_rescore_queue_priority() -> Result<(), StoreError> {
    let root = temp_root();
    std::fs::create_dir_all(&root).map_err(io_to_store_error)?;
    let db_path = root.join("mail.sqlite");
    let data_root = root.join("data");
    {
        let connection = Connection::open(&db_path).map_err(sql_to_store_error)?;
        connection
            .execute_batch(
                "CREATE TABLE cache_rescore_queue (
                    account_id TEXT NOT NULL,
                    message_id TEXT NOT NULL,
                    reason TEXT NOT NULL,
                    queued_at TEXT NOT NULL,
                    PRIMARY KEY (account_id, message_id)
                );",
            )
            .map_err(sql_to_store_error)?;
    }

    let store = DatabaseStore::open(&db_path, data_root)?;
    let connection = store.read_connection()?;
    let mut statement = connection
        .prepare("PRAGMA table_info(cache_rescore_queue)")
        .map_err(sql_to_store_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sql_to_store_error)?;
    let mut has_rescore_priority = false;
    for column in columns {
        has_rescore_priority |= column.map_err(sql_to_store_error)? == "rescore_priority";
    }

    assert!(has_rescore_priority);
    Ok(())
}

// spec: docs/L1-sync#cache-stale-rescore
#[test]
fn stale_cache_objects_are_queued_for_rescore_in_bounded_oldest_first_batches(
) -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    for message_id in ["oldest", "middle", "fresh"] {
        insert_message_metadata(&store, message_id, "2026-04-27T00:00:00Z")?;
        store.upsert_cache_candidates(&[candidate(message_id, 1.0, 4096)])?;
    }
    set_last_scored_at(&store, "oldest", "2026-04-20T00:00:00Z")?;
    set_last_scored_at(&store, "middle", "2026-04-21T00:00:00Z")?;
    set_last_scored_at(&store, "fresh", "2026-04-27T00:00:00Z")?;

    let queued = store.queue_stale_cache_rescore_candidates(&account, "2026-04-22T00:00:00Z", 1)?;
    let candidates = store.list_cache_rescore_candidates(&account, 10)?;

    assert_eq!(queued, 1);
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.message_id.as_str())
            .collect::<Vec<_>>(),
        vec!["oldest"]
    );
    assert_eq!(candidates[0].signal_reason, "stale-periodic");
    Ok(())
}

// spec: docs/L1-sync#cache-stale-rescore
#[test]
fn stale_rescore_queue_skips_already_queued_and_fetching_objects() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    for message_id in ["already-queued", "fetching", "stale"] {
        insert_message_metadata(&store, message_id, "2026-04-27T00:00:00Z")?;
        store.upsert_cache_candidates(&[candidate(message_id, 1.0, 4096)])?;
        set_last_scored_at(&store, message_id, "2026-04-20T00:00:00Z")?;
    }
    store.record_cache_signal_updates(&[CacheSignalUpdate {
        account_id: "primary".to_string(),
        message_id: "already-queued".to_string(),
        reason: "search-visible".to_string(),
        search: None,
        thread_activity: None,
        sender_affinity: None,
        local_behavior: None,
        direct_user_boost: Some(0.8),
        pinned: None,
    }])?;
    store.mark_cache_object_state(
        &account,
        &MessageId::from("fetching"),
        CacheLayer::Body,
        None,
        CacheObjectState::Fetching,
        None,
    )?;

    let queued =
        store.queue_stale_cache_rescore_candidates(&account, "2026-04-22T00:00:00Z", 10)?;
    let mut candidates = store
        .list_cache_rescore_candidates(&account, 10)?
        .into_iter()
        .map(|candidate| (candidate.message_id, candidate.signal_reason))
        .collect::<Vec<_>>();
    candidates.sort_unstable_by(|left, right| left.0.cmp(&right.0));

    assert_eq!(queued, 1);
    assert_eq!(
        candidates,
        vec![
            ("already-queued".to_string(), "search-visible".to_string()),
            ("stale".to_string(), "stale-periodic".to_string()),
        ]
    );
    Ok(())
}
