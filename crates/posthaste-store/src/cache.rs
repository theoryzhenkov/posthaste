use super::*;

const BODY_CACHE_OBJECT_ID: &str = "";
const BODY_STRUCTURAL_REPAIR_REASON: &str = "body-structural";
pub(crate) const BACKGROUND_RESCORE_PRIORITY: f64 = 0.0;
const BACKGROUND_RESCORE_PRIORITY_CEILING: f64 = 99.0;

fn cache_object_id_key(object_id: Option<&str>) -> &str {
    object_id.unwrap_or("")
}

fn u64_to_i64(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::Failure("cache byte count too large".to_string()))
}

fn i64_to_u64(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Failure("negative cache byte count".to_string()))
}

fn parse_cache_layer(value: String) -> Result<CacheLayer, rusqlite::Error> {
    CacheLayer::parse(&value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown cache layer {value}"),
            )),
        )
    })
}

fn parse_cache_fetch_unit(value: String) -> Result<CacheFetchUnit, rusqlite::Error> {
    CacheFetchUnit::parse(&value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown cache fetch unit {value}"),
            )),
        )
    })
}

fn parse_cache_object_state(value: String) -> Result<CacheObjectState, rusqlite::Error> {
    CacheObjectState::parse(&value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown cache object state {value}"),
            )),
        )
    })
}

fn option_u64_to_i64(value: Option<u64>) -> Result<Option<i64>, StoreError> {
    value.map(u64_to_i64).transpose()
}

fn optional_i64_to_u64(value: Option<i64>, column: usize) -> Result<Option<u64>, rusqlite::Error> {
    value.map(i64_to_u64).transpose().map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            )),
        )
    })
}

fn body_exists_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<bool, StoreError> {
    tx.query_row(
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
    tx.execute(
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
    tx.execute(
        "INSERT INTO cache_rescore_queue (
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
            rescore_priority = MAX(cache_rescore_queue.rescore_priority, excluded.rescore_priority)",
        params![
            account_id.as_str(),
            message_id.as_str(),
            reason,
            now.as_str(),
            finite_rescore_priority(rescore_priority),
        ],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

fn finite_rescore_priority(priority: f64) -> f64 {
    if priority.is_finite() {
        priority.max(0.0)
    } else {
        0.0
    }
}

pub(crate) fn repair_missing_body_cache_objects(connection: &Connection) -> Result<(), StoreError> {
    let now = now_iso8601()?;
    let pruned_queue = connection
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
    let pruned_signals = connection
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
    let pruned_objects = connection
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
        debug!(
            pruned_queue,
            pruned_signals, pruned_objects, "pruned orphan cache child rows"
        );
    }
    connection
        .execute(
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
                rescore_priority = MAX(cache_rescore_queue.rescore_priority, excluded.rescore_priority)",
            params![
                BODY_STRUCTURAL_REPAIR_REASON,
                now.as_str(),
                BACKGROUND_RESCORE_PRIORITY
            ],
        )
        .map_err(sql_to_store_error)?;
    let repaired = connection
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
        debug!(repaired, "repaired missing structural body cache objects");
    }
    Ok(())
}

impl CacheStore for DatabaseStore {
    fn upsert_cache_candidates(&self, candidates: &[CacheCandidate]) -> Result<(), StoreError> {
        if candidates.is_empty() {
            return Ok(());
        }
        let now = now_iso8601()?;
        self.write_transaction(|tx| {
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

    fn record_cache_signal_updates(&self, updates: &[CacheSignalUpdate]) -> Result<(), StoreError> {
        if updates.is_empty() {
            return Ok(());
        }
        let now = now_iso8601()?;
        self.write_transaction(|tx| {
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

    fn list_cache_rescore_candidates(
        &self,
        account_id: &AccountId,
        limit: usize,
    ) -> Result<Vec<CacheRescoreCandidate>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let connection = self.read_connection()?;
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

    fn queue_stale_cache_rescore_candidates(
        &self,
        account_id: &AccountId,
        stale_before: &str,
        limit: usize,
    ) -> Result<usize, StoreError> {
        if limit == 0 {
            return Ok(0);
        }
        let now = now_iso8601()?;
        self.write_transaction(|tx| {
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

    fn update_cache_priorities(&self, updates: &[CachePriorityUpdate]) -> Result<(), StoreError> {
        if updates.is_empty() {
            return Ok(());
        }
        let now = now_iso8601()?;
        self.write_transaction(|tx| {
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

    fn list_cache_fetch_candidates(
        &self,
        account_id: &AccountId,
        layer: CacheLayer,
        limit: usize,
    ) -> Result<Vec<CacheFetchCandidate>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let connection = self.read_connection()?;
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

    fn mark_cache_object_state(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        layer: CacheLayer,
        object_id: Option<&str>,
        state: CacheObjectState,
        error_code: Option<&str>,
    ) -> Result<(), StoreError> {
        let now = now_iso8601()?;
        self.write_transaction(|tx| {
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

    fn cache_used_bytes(&self) -> Result<u64, StoreError> {
        let connection = self.read_connection()?;
        let used: i64 = connection
            .query_row(
                "SELECT COALESCE(SUM(fetch_bytes), 0) FROM cache_object WHERE state = 'cached'",
                [],
                |row| row.get(0),
            )
            .map_err(sql_to_store_error)?;
        i64_to_u64(used)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("posthaste-store-cache-test-{now}-{seq}"))
    }

    fn candidate(message_id: &str, priority: f64, fetch_bytes: u64) -> CacheCandidate {
        CacheCandidate {
            account_id: "primary".to_string(),
            message_id: message_id.to_string(),
            layer: CacheLayer::Body,
            object_id: None,
            fetch_unit: CacheFetchUnit::BodyOnly,
            value_bytes: fetch_bytes,
            fetch_bytes,
            priority,
            reason: "test".to_string(),
        }
    }

    fn insert_message_metadata(
        store: &DatabaseStore,
        message_id: &str,
        received_at: &str,
    ) -> Result<(), StoreError> {
        store.write_transaction(|tx| {
            tx.execute(
                "INSERT OR IGNORE INTO mailbox (account_id, id, name, role)
                 VALUES ('primary', 'inbox', 'Inbox', 'inbox')",
                [],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "INSERT INTO message (
                    account_id, id, thread_id, received_at, size, is_read, is_flagged
                 ) VALUES ('primary', ?1, 'thread-1', ?2, 4096, 0, 1)",
                params![message_id, received_at],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "INSERT INTO message_mailbox (account_id, message_id, mailbox_id)
                 VALUES ('primary', ?1, 'inbox')",
                params![message_id],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    fn set_last_scored_at(
        store: &DatabaseStore,
        message_id: &str,
        last_scored_at: &str,
    ) -> Result<(), StoreError> {
        store.write_transaction(|tx| {
            tx.execute(
                "UPDATE cache_object
                 SET last_scored_at = ?2
                 WHERE account_id = 'primary' AND message_id = ?1",
                params![message_id, last_scored_at],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

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

    #[test]
    fn cache_signal_updates_queue_rescore_candidates_with_search_signals() -> Result<(), StoreError>
    {
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

        let queued =
            store.queue_stale_cache_rescore_candidates(&account, "2026-04-22T00:00:00Z", 1)?;
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
}
