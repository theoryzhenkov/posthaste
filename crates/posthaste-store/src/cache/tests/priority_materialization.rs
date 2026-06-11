use super::*;

#[test]
fn priority_updates_requeue_failed_candidates_as_wanted() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    insert_message_metadata(&store, "message-1", "2026-04-27T00:00:00Z")?;
    store.upsert_cache_candidates(&[candidate("message-1", 0.5, 4096)])?;
    store.mark_cache_object_state(
        &account,
        &MessageId::from("message-1"),
        CacheLayer::Body,
        None,
        CacheObjectState::Failed,
        Some("network_error"),
    )?;

    store.update_cache_priorities(&[CachePriorityUpdate {
        account_id: "primary".to_string(),
        message_id: "message-1".to_string(),
        layer: CacheLayer::Body,
        object_id: None,
        fetch_unit: CacheFetchUnit::BodyOnly,
        value_bytes: 4096,
        fetch_bytes: 4096,
        priority: 2.0,
        reason: "search-visible".to_string(),
    }])?;

    let candidates = store.list_cache_fetch_candidates(&account, CacheLayer::Body, 10)?;

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].message_id, "message-1");
    assert_eq!(candidates[0].priority, 2.0);
    Ok(())
}

#[test]
fn signal_updates_materialize_missing_body_cache_objects() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    insert_message_metadata(&store, "missing", "2026-04-27T00:00:00Z")?;
    insert_message_metadata(&store, "wanted", "2026-04-27T00:00:00Z")?;
    store.upsert_cache_candidates(&[candidate("wanted", 0.5, 4096)])?;

    store.record_cache_signal_updates(&[
        CacheSignalUpdate {
            account_id: "primary".to_string(),
            message_id: "missing".to_string(),
            reason: "search-visible".to_string(),
            search: None,
            thread_activity: None,
            sender_affinity: None,
            local_behavior: None,
            direct_user_boost: Some(0.8),
            pinned: None,
        },
        CacheSignalUpdate {
            account_id: "primary".to_string(),
            message_id: "wanted".to_string(),
            reason: "search-visible".to_string(),
            search: None,
            thread_activity: None,
            sender_affinity: None,
            local_behavior: None,
            direct_user_boost: Some(0.8),
            pinned: None,
        },
    ])?;

    let candidates = store.list_cache_rescore_candidates(&account, 10)?;

    let mut message_ids = candidates
        .iter()
        .map(|candidate| candidate.message_id.as_str())
        .collect::<Vec<_>>();
    message_ids.sort_unstable();
    assert_eq!(message_ids, vec!["missing", "wanted"]);
    Ok(())
}
