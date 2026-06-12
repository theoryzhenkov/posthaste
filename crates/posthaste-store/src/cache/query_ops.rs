use super::*;

pub(super) fn list_cache_rescore_candidates(
    store: &DatabaseStore,
    account_id: &AccountId,
    limit: usize,
) -> Result<Vec<CacheRescoreCandidate>, StoreError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let connection = store.read_connection()?;
    let mut statement = connection
        .prepare(
            "WITH queued AS (
                SELECT account_id, message_id, reason, queued_at, rescore_priority
                FROM cache_rescore_queue
                WHERE account_id = ?1
                ORDER BY rescore_priority DESC, queued_at ASC, message_id ASC
                LIMIT ?2
             )
             SELECT
                co.account_id,
                co.message_id,
                co.layer,
                co.object_id,
                co.fetch_unit,
                co.state,
                co.value_bytes,
                co.fetch_bytes,
                co.priority,
                m.size,
                m.has_attachment,
                m.received_at,
                EXISTS (
                    SELECT 1
                    FROM message_mailbox mm
                    JOIN mailbox mb
                      ON mb.account_id = mm.account_id
                     AND mb.id = mm.mailbox_id
                    WHERE mm.account_id = m.account_id
                      AND mm.message_id = m.id
                      AND mb.role = 'inbox'
                ) AS in_inbox,
                m.is_read,
                m.is_flagged,
                COALESCE(cms.thread_activity_score, 0),
                COALESCE(cms.sender_affinity_score, 0),
                COALESCE(cms.local_behavior_score, 0),
                cms.search_total_messages,
                cms.search_result_count,
                cms.search_result_rank,
                COALESCE(cms.direct_user_boost, 0),
                COALESCE(cms.pinned, 0),
                queued.reason,
                queued.rescore_priority
             FROM queued
             JOIN cache_object co
               ON co.account_id = queued.account_id
              AND co.message_id = queued.message_id
             JOIN message m
               ON m.account_id = co.account_id
              AND m.id = co.message_id
             LEFT JOIN cache_message_signal cms
               ON cms.account_id = co.account_id
              AND cms.message_id = co.message_id
             ORDER BY queued.rescore_priority DESC, queued.queued_at ASC, co.priority DESC",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![account_id.as_str(), limit as i64], |row| {
            let object_id: String = row.get(3)?;
            let search_total_messages = optional_i64_to_u64(row.get(18)?, 18)?;
            let search_result_count = optional_i64_to_u64(row.get(19)?, 19)?;
            let search_result_rank = optional_i64_to_u64(row.get(20)?, 20)?;
            let search = match (
                search_total_messages,
                search_result_count,
                search_result_rank,
            ) {
                (Some(total_messages), Some(result_count), Some(result_rank)) => {
                    Some(CacheSearchSignals {
                        total_messages,
                        result_count,
                        result_rank,
                    })
                }
                _ => None,
            };
            Ok(CacheRescoreCandidate {
                account_id: row.get(0)?,
                message_id: row.get(1)?,
                layer: parse_cache_layer(row.get(2)?)?,
                object_id: if object_id.is_empty() {
                    None
                } else {
                    Some(object_id)
                },
                fetch_unit: parse_cache_fetch_unit(row.get(4)?)?,
                state: parse_cache_object_state(row.get(5)?)?,
                value_bytes: i64_to_u64(row.get(6)?).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        6,
                        rusqlite::types::Type::Integer,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            err.to_string(),
                        )),
                    )
                })?,
                fetch_bytes: i64_to_u64(row.get(7)?).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        7,
                        rusqlite::types::Type::Integer,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            err.to_string(),
                        )),
                    )
                })?,
                priority: row.get(8)?,
                message_size: row.get(9)?,
                has_attachment: row.get::<_, i64>(10)? != 0,
                received_at: row.get(11)?,
                in_inbox: row.get::<_, i64>(12)? != 0,
                unread: row.get::<_, i64>(13)? == 0,
                flagged: row.get::<_, i64>(14)? != 0,
                thread_activity: row.get(15)?,
                sender_affinity: row.get(16)?,
                local_behavior: row.get(17)?,
                search,
                direct_user_boost: row.get(21)?,
                pinned: row.get::<_, i64>(22)? != 0,
                signal_reason: row.get(23)?,
                rescore_priority: row.get(24)?,
            })
        })
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}
pub(super) fn queue_stale_cache_rescore_candidates(
    store: &DatabaseStore,
    account_id: &AccountId,
    stale_before: &str,
    limit: usize,
) -> Result<usize, StoreError> {
    if limit == 0 {
        return Ok(0);
    }
    let now = now_iso8601()?;
    store.write_transaction(|tx| {
        let queued = tx
            .execute(
                "WITH stale AS (
                    SELECT
                        co.account_id,
                        co.message_id,
                        MIN(co.last_scored_at) AS oldest_scored_at,
                        MAX(co.priority) AS highest_priority
                    FROM cache_object co
                    WHERE co.account_id = ?1
                      AND co.last_scored_at < ?2
                      AND co.state <> 'fetching'
                      AND NOT EXISTS (
                        SELECT 1
                        FROM cache_rescore_queue q
                        WHERE q.account_id = co.account_id
                          AND q.message_id = co.message_id
                      )
                    GROUP BY co.account_id, co.message_id
                    ORDER BY oldest_scored_at ASC, highest_priority DESC
                    LIMIT ?3
                 )
                 INSERT INTO cache_rescore_queue (
                    account_id, message_id, reason, queued_at, rescore_priority
                 )
                 SELECT
                    account_id,
                    message_id,
                    'stale-periodic',
                    ?4,
                    CASE
                        WHEN highest_priority > ?5 THEN ?5
                        WHEN highest_priority > 0 THEN highest_priority
                        ELSE 0
                    END
                 FROM stale",
                params![
                    account_id.as_str(),
                    stale_before,
                    limit as i64,
                    now.as_str(),
                    BACKGROUND_RESCORE_PRIORITY_CEILING,
                ],
            )
            .map_err(sql_to_store_error)?;
        Ok(queued)
    })
}
pub(super) fn list_cache_fetch_candidates(
    store: &DatabaseStore,
    account_id: &AccountId,
    layer: CacheLayer,
    limit: usize,
) -> Result<Vec<CacheFetchCandidate>, StoreError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let connection = store.read_connection()?;
    let mut statement = connection
        .prepare(
            "SELECT account_id, message_id, layer, object_id, fetch_unit, fetch_bytes, priority
             FROM cache_object
             WHERE account_id = ?1 AND layer = ?2 AND state = 'wanted' AND fetch_bytes > 0
             ORDER BY priority DESC, last_scored_at ASC
             LIMIT ?3",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(
            params![account_id.as_str(), layer.as_str(), limit as i64],
            |row| {
                let object_id: String = row.get(3)?;
                Ok(CacheFetchCandidate {
                    account_id: row.get(0)?,
                    message_id: row.get(1)?,
                    layer: parse_cache_layer(row.get(2)?)?,
                    object_id: if object_id.is_empty() {
                        None
                    } else {
                        Some(object_id)
                    },
                    fetch_unit: parse_cache_fetch_unit(row.get(4)?)?,
                    fetch_bytes: i64_to_u64(row.get(5)?).map_err(|err| {
                        rusqlite::Error::FromSqlConversionFailure(
                            5,
                            rusqlite::types::Type::Integer,
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                err.to_string(),
                            )),
                        )
                    })?,
                    priority: row.get(6)?,
                })
            },
        )
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}
