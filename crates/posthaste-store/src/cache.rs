use super::*;

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
    CacheLayer::from_str(&value).ok_or_else(|| {
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
    CacheFetchUnit::from_str(&value).ok_or_else(|| {
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
                 WHERE account_id = ?1 AND layer = ?2 AND state = 'wanted'
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

    #[test]
    fn cache_ledger_returns_wanted_candidates_by_priority() -> Result<(), StoreError> {
        let root = temp_root();
        let store = DatabaseStore::open(root.join("mail.sqlite"), root.join("data"))?;
        let account = AccountId::from("primary");
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
}
