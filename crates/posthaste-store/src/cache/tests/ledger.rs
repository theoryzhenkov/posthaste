use super::*;

#[test]
fn cache_ledger_returns_wanted_candidates_by_priority() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    insert_message_metadata(&store, "low", "2026-04-27T00:00:00Z")?;
    insert_message_metadata(&store, "high", "2026-04-27T00:00:01Z")?;
    insert_message_metadata(&store, "middle", "2026-04-27T00:00:02Z")?;
    store.upsert_cache_candidates(&[
        candidate("low", 0.5, 100),
        candidate("high", 2.0, 200),
        candidate("middle", 1.0, 150),
    ])?;

    let candidates = store.list_cache_fetch_candidates(&account, CacheLayer::Body, 2)?;

    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.message_id.as_str())
            .collect::<Vec<_>>(),
        vec!["high", "middle"]
    );
    Ok(())
}

#[test]
fn cache_ledger_tracks_cached_bytes() -> Result<(), StoreError> {
    let root = temp_root();
    let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
    let account = AccountId::from("primary");
    insert_message_metadata(&store, "one", "2026-04-27T00:00:00Z")?;
    insert_message_metadata(&store, "two", "2026-04-27T00:00:01Z")?;
    store.upsert_cache_candidates(&[candidate("one", 1.0, 128), candidate("two", 2.0, 256)])?;

    store.mark_cache_object_state(
        &account,
        &MessageId::from("two"),
        CacheLayer::Body,
        None,
        CacheObjectState::Cached,
        None,
    )?;

    assert_eq!(store.cache_used_bytes()?, 256);
    assert_eq!(
        store
            .list_cache_fetch_candidates(&account, CacheLayer::Body, 10)?
            .len(),
        1
    );
    Ok(())
}
