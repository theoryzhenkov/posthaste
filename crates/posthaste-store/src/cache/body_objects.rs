use super::*;
use crate::sql_cache::CachedSql;

const CACHE_RESCORE_QUEUE_UPSERT_SQL: &str = "
INSERT INTO cache_rescore_queue (
    account_id, message_id, reason, queued_at, rescore_priority
) VALUES (?1, ?2, ?3, ?4, ?5)
ON CONFLICT(account_id, message_id) DO UPDATE SET
    reason = CASE
        WHEN excluded.rescore_priority >= cache_rescore_queue.rescore_priority
        THEN excluded.reason
        ELSE cache_rescore_queue.reason
    END,
    queued_at = CASE
        WHEN excluded.rescore_priority >= cache_rescore_queue.rescore_priority
        THEN excluded.queued_at
        ELSE cache_rescore_queue.queued_at
    END,
    rescore_priority = MAX(cache_rescore_queue.rescore_priority, excluded.rescore_priority)";

pub(super) fn body_exists_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<bool, StoreError> {
    tx.query_row_cached(
        "SELECT 1 FROM message_body WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
        |_row| Ok(()),
    )
    .optional()
    .map_err(sql_to_store_error)
    .map(|row| row.is_some())
}

pub(crate) fn ensure_body_cache_object_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    body_cached_hint: bool,
    reason: &str,
    rescore_priority: f64,
) -> Result<(), StoreError> {
    let now = now_iso8601()?;
    let body_cached = body_cached_hint || body_exists_tx(tx, account_id, message_id)?;
    let state = if body_cached {
        CacheObjectState::Cached
    } else {
        CacheObjectState::Wanted
    };
    let fetched_at = body_cached.then_some(now.as_str());
    tx.execute_cached(
        "INSERT INTO cache_object (
            account_id, message_id, layer, object_id, fetch_unit, state,
            value_bytes, fetch_bytes, priority, reason, last_scored_at, fetched_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 0, 0, ?7, ?8, ?9)
         ON CONFLICT(account_id, message_id, layer, object_id) DO UPDATE SET
            state = CASE
                WHEN excluded.state = 'cached' THEN 'cached'
                ELSE cache_object.state
            END,
            fetched_at = CASE
                WHEN excluded.state = 'cached' THEN COALESCE(cache_object.fetched_at, excluded.fetched_at)
                ELSE cache_object.fetched_at
            END,
            error_code = CASE
                WHEN excluded.state = 'cached' THEN NULL
                ELSE cache_object.error_code
            END",
        params![
            account_id.as_str(),
            message_id.as_str(),
            CacheLayer::Body.as_str(),
            BODY_CACHE_OBJECT_ID,
            CacheFetchUnit::BodyOnly.as_str(),
            state.as_str(),
            reason,
            now.as_str(),
            fetched_at,
        ],
    )
    .map_err(sql_to_store_error)?;
    upsert_cache_rescore_queue_tx(
        tx,
        account_id,
        message_id,
        reason,
        now.as_str(),
        rescore_priority,
    )?;
    Ok(())
}

pub(super) fn upsert_cache_rescore_queue_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    reason: &str,
    queued_at: &str,
    rescore_priority: f64,
) -> Result<(), StoreError> {
    tx.execute_cached(
        CACHE_RESCORE_QUEUE_UPSERT_SQL,
        params![
            account_id.as_str(),
            message_id.as_str(),
            reason,
            queued_at,
            finite_rescore_priority(rescore_priority),
        ],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

pub(super) fn finite_rescore_priority(priority: f64) -> f64 {
    if priority.is_finite() {
        priority.max(0.0)
    } else {
        0.0
    }
}

pub(crate) fn repair_missing_body_cache_objects(
    connection: &mut Connection,
) -> Result<(), StoreError> {
    let tx = connection.transaction().map_err(sql_to_store_error)?;
    repair_missing_body_cache_objects_tx(&tx)?;
    tx.commit().map_err(sql_to_store_error)?;
    Ok(())
}

pub(super) fn repair_missing_body_cache_objects_tx(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let now = now_iso8601()?;
    let pruned_queue = tx
        .execute(
            "DELETE FROM cache_rescore_queue
             WHERE NOT EXISTS (
                SELECT 1
                FROM message m
                WHERE m.account_id = cache_rescore_queue.account_id
                  AND m.id = cache_rescore_queue.message_id
             )",
            [],
        )
        .map_err(sql_to_store_error)?;
    let pruned_signals = tx
        .execute(
            "DELETE FROM cache_message_signal
             WHERE NOT EXISTS (
                SELECT 1
                FROM message m
                WHERE m.account_id = cache_message_signal.account_id
                  AND m.id = cache_message_signal.message_id
             )",
            [],
        )
        .map_err(sql_to_store_error)?;
    let pruned_objects = tx
        .execute(
            "DELETE FROM cache_object
             WHERE NOT EXISTS (
                SELECT 1
                FROM message m
                WHERE m.account_id = cache_object.account_id
                  AND m.id = cache_object.message_id
             )",
            [],
        )
        .map_err(sql_to_store_error)?;
    if pruned_queue > 0 || pruned_signals > 0 || pruned_objects > 0 {
        ph_debug!(
            events::STORE_CACHE_ORPHANS_PRUNED,
            pruned_queue,
            pruned_signals,
            pruned_objects,
            "pruned orphan cache child rows"
        );
    }
    let sql = format!(
        "INSERT INTO cache_rescore_queue (
            account_id, message_id, reason, queued_at, rescore_priority
         )
         SELECT m.account_id, m.id, ?1, ?2, ?3
         FROM message m
         WHERE NOT EXISTS (
            SELECT 1
            FROM cache_object co
            WHERE co.account_id = m.account_id
              AND co.message_id = m.id
              AND co.layer = 'body'
              AND co.object_id = ''
         )
         {CACHE_RESCORE_QUEUE_UPSERT_UPDATE_SQL}"
    );
    tx.execute(
        &sql,
        params![
            BODY_STRUCTURAL_REPAIR_REASON,
            now.as_str(),
            BACKGROUND_RESCORE_PRIORITY
        ],
    )
    .map_err(sql_to_store_error)?;
    let repaired = tx
        .execute(
            "INSERT INTO cache_object (
                account_id, message_id, layer, object_id, fetch_unit, state,
                value_bytes, fetch_bytes, priority, reason, last_scored_at, fetched_at
             )
             SELECT
                m.account_id,
                m.id,
                'body',
                '',
                'body_only',
                CASE WHEN mb.message_id IS NULL THEN 'wanted' ELSE 'cached' END,
                0,
                0,
                0,
                ?1,
                ?2,
                CASE WHEN mb.message_id IS NULL THEN NULL ELSE ?2 END
             FROM message m
             LEFT JOIN message_body mb
               ON mb.account_id = m.account_id
              AND mb.message_id = m.id
             WHERE NOT EXISTS (
                SELECT 1
                FROM cache_object co
                WHERE co.account_id = m.account_id
                  AND co.message_id = m.id
                  AND co.layer = 'body'
                  AND co.object_id = ''
             )",
            params![BODY_STRUCTURAL_REPAIR_REASON, now.as_str()],
        )
        .map_err(sql_to_store_error)?;
    if repaired > 0 {
        ph_debug!(
            events::STORE_CACHE_STRUCTURAL_BODY_REPAIRED,
            repaired,
            "repaired missing structural body cache objects"
        );
    }
    Ok(())
}
