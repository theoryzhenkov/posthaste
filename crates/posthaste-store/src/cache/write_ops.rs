use super::*;

pub(super) fn upsert_cache_candidates(
    store: &DatabaseStore,
    candidates: &[CacheCandidate],
) -> Result<(), StoreError> {
    if candidates.is_empty() {
        return Ok(());
    }
    let now = now_iso8601()?;
    store.write_transaction(|tx| {
        for candidate in candidates {
            tx.execute(
                "INSERT INTO cache_object (
                    account_id, message_id, layer, object_id, fetch_unit, state,
                    value_bytes, fetch_bytes, priority, reason, last_scored_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                ON CONFLICT(account_id, message_id, layer, object_id) DO UPDATE SET
                    fetch_unit = excluded.fetch_unit,
                    state = CASE
                        WHEN cache_object.state = 'cached' THEN cache_object.state
                        ELSE excluded.state
                    END,
                    value_bytes = excluded.value_bytes,
                    fetch_bytes = excluded.fetch_bytes,
                    priority = excluded.priority,
                    reason = excluded.reason,
                    last_scored_at = excluded.last_scored_at,
                    error_code = CASE
                        WHEN cache_object.state = 'cached' THEN cache_object.error_code
                        ELSE NULL
                    END",
                params![
                    candidate.account_id.as_str(),
                    candidate.message_id.as_str(),
                    candidate.layer.as_str(),
                    cache_object_id_key(candidate.object_id.as_deref()),
                    candidate.fetch_unit.as_str(),
                    CacheObjectState::Wanted.as_str(),
                    u64_to_i64(candidate.value_bytes)?,
                    u64_to_i64(candidate.fetch_bytes)?,
                    candidate.priority,
                    candidate.reason.as_str(),
                    now.as_str(),
                ],
            )
            .map_err(sql_to_store_error)?;
        }
        Ok(())
    })
}
pub(super) fn record_cache_signal_updates(
    store: &DatabaseStore,
    updates: &[CacheSignalUpdate],
) -> Result<(), StoreError> {
    if updates.is_empty() {
        return Ok(());
    }
    let now = now_iso8601()?;
    store.write_transaction(|tx| {
        for update in updates {
            let search_total_messages =
                option_u64_to_i64(update.search.as_ref().map(|search| search.total_messages))?;
            let search_result_count =
                option_u64_to_i64(update.search.as_ref().map(|search| search.result_count))?;
            let search_result_rank =
                option_u64_to_i64(update.search.as_ref().map(|search| search.result_rank))?;
            let pinned = update.pinned.map(bool_to_i64);
            tx.execute(
                "INSERT INTO cache_message_signal (
                    account_id, message_id,
                    search_total_messages, search_result_count, search_result_rank,
                    search_seen_count, last_search_seen_at,
                    thread_activity_score, sender_affinity_score, local_behavior_score,
                    direct_user_boost, pinned, dirty_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                ON CONFLICT(account_id, message_id) DO UPDATE SET
                    search_total_messages = COALESCE(excluded.search_total_messages, cache_message_signal.search_total_messages),
                    search_result_count = COALESCE(excluded.search_result_count, cache_message_signal.search_result_count),
                    search_result_rank = COALESCE(excluded.search_result_rank, cache_message_signal.search_result_rank),
                    search_seen_count = cache_message_signal.search_seen_count + excluded.search_seen_count,
                    last_search_seen_at = COALESCE(excluded.last_search_seen_at, cache_message_signal.last_search_seen_at),
                    thread_activity_score = COALESCE(excluded.thread_activity_score, cache_message_signal.thread_activity_score),
                    sender_affinity_score = COALESCE(excluded.sender_affinity_score, cache_message_signal.sender_affinity_score),
                    local_behavior_score = COALESCE(excluded.local_behavior_score, cache_message_signal.local_behavior_score),
                    direct_user_boost = COALESCE(excluded.direct_user_boost, cache_message_signal.direct_user_boost),
                    pinned = COALESCE(excluded.pinned, cache_message_signal.pinned),
                    dirty_at = excluded.dirty_at",
                params![
                    update.account_id.as_str(),
                    update.message_id.as_str(),
                    search_total_messages,
                    search_result_count,
                    search_result_rank,
                    if update.search.is_some() { 1_i64 } else { 0_i64 },
                    update.search.as_ref().map(|_| now.as_str()),
                    update.thread_activity,
                    update.sender_affinity,
                    update.local_behavior,
                    update.direct_user_boost,
                    pinned,
                    now.as_str(),
                ],
            )
            .map_err(sql_to_store_error)?;
            ensure_body_cache_object_tx(
                tx,
                &AccountId::from(update.account_id.as_str()),
                &MessageId::from(update.message_id.as_str()),
                false,
                update.reason.as_str(),
                cache_signal_rescore_priority(update),
            )?;
        }
        Ok(())
    })
}
pub(super) fn update_cache_priorities(
    store: &DatabaseStore,
    updates: &[CachePriorityUpdate],
) -> Result<(), StoreError> {
    if updates.is_empty() {
        return Ok(());
    }
    let now = now_iso8601()?;
    store.write_transaction(|tx| {
        for update in updates {
            tx.execute(
                "UPDATE cache_object
                 SET fetch_unit = ?5,
                     value_bytes = ?6,
                     fetch_bytes = ?7,
                     priority = ?8,
                     reason = ?9,
                     last_scored_at = ?10,
                     state = CASE
                        WHEN state IN ('cached', 'fetching') THEN state
                        ELSE 'wanted'
                     END,
                     error_code = CASE
                        WHEN state = 'cached' THEN error_code
                        ELSE NULL
                     END
                 WHERE account_id = ?1
                   AND message_id = ?2
                   AND layer = ?3
                   AND object_id = ?4",
                params![
                    update.account_id.as_str(),
                    update.message_id.as_str(),
                    update.layer.as_str(),
                    cache_object_id_key(update.object_id.as_deref()),
                    update.fetch_unit.as_str(),
                    u64_to_i64(update.value_bytes)?,
                    u64_to_i64(update.fetch_bytes)?,
                    update.priority,
                    update.reason.as_str(),
                    now.as_str(),
                ],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM cache_rescore_queue
                 WHERE account_id = ?1 AND message_id = ?2",
                params![update.account_id.as_str(), update.message_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
        }
        Ok(())
    })
}
pub(super) fn mark_cache_object_state(
    store: &DatabaseStore,
    account_id: &AccountId,
    message_id: &MessageId,
    layer: CacheLayer,
    object_id: Option<&str>,
    state: CacheObjectState,
    error_code: Option<&str>,
) -> Result<(), StoreError> {
    let now = now_iso8601()?;
    store.write_transaction(|tx| {
        tx.execute(
            "UPDATE cache_object
             SET state = ?5,
                 fetched_at = CASE WHEN ?5 = 'cached' THEN ?6 ELSE fetched_at END,
                 error_code = ?7
             WHERE account_id = ?1 AND message_id = ?2 AND layer = ?3 AND object_id = ?4",
            params![
                account_id.as_str(),
                message_id.as_str(),
                layer.as_str(),
                cache_object_id_key(object_id),
                state.as_str(),
                now.as_str(),
                error_code,
            ],
        )
        .map_err(sql_to_store_error)?;
        Ok(())
    })
}
pub(super) fn cache_used_bytes(store: &DatabaseStore) -> Result<u64, StoreError> {
    let connection = store.read_connection()?;
    let used: i64 = connection
        .query_row(
            "SELECT COALESCE(SUM(fetch_bytes), 0) FROM cache_object WHERE state = 'cached'",
            [],
            |row| row.get(0),
        )
        .map_err(sql_to_store_error)?;
    i64_to_u64(used)
}
