use super::*;

mod body_objects;
mod helpers;
mod query_ops;
mod write_ops;

use helpers::*;

pub(crate) use body_objects::{ensure_body_cache_object_tx, repair_missing_body_cache_objects};

const BODY_CACHE_OBJECT_ID: &str = "";
const BODY_STRUCTURAL_REPAIR_REASON: &str = "body-structural";
pub(crate) const BACKGROUND_RESCORE_PRIORITY: f64 = 0.0;
const BACKGROUND_RESCORE_PRIORITY_CEILING: f64 = 99.0;
const CACHE_RESCORE_QUEUE_UPSERT_UPDATE_SQL: &str = "
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

impl CacheStore for DatabaseStore {
    fn upsert_cache_candidates(&self, candidates: &[CacheCandidate]) -> Result<(), StoreError> {
        write_ops::upsert_cache_candidates(self, candidates)
    }

    fn record_cache_signal_updates(&self, updates: &[CacheSignalUpdate]) -> Result<(), StoreError> {
        write_ops::record_cache_signal_updates(self, updates)
    }

    fn list_cache_rescore_candidates(
        &self,
        account_id: &AccountId,
        limit: usize,
    ) -> Result<Vec<CacheRescoreCandidate>, StoreError> {
        query_ops::list_cache_rescore_candidates(self, account_id, limit)
    }

    fn queue_stale_cache_rescore_candidates(
        &self,
        account_id: &AccountId,
        stale_before: &str,
        limit: usize,
    ) -> Result<usize, StoreError> {
        query_ops::queue_stale_cache_rescore_candidates(self, account_id, stale_before, limit)
    }

    fn update_cache_priorities(&self, updates: &[CachePriorityUpdate]) -> Result<(), StoreError> {
        write_ops::update_cache_priorities(self, updates)
    }

    fn list_cache_fetch_candidates(
        &self,
        account_id: &AccountId,
        layer: CacheLayer,
        limit: usize,
    ) -> Result<Vec<CacheFetchCandidate>, StoreError> {
        query_ops::list_cache_fetch_candidates(self, account_id, layer, limit)
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
        write_ops::mark_cache_object_state(
            self, account_id, message_id, layer, object_id, state, error_code,
        )
    }

    fn cache_used_bytes(&self) -> Result<u64, StoreError> {
        write_ops::cache_used_bytes(self)
    }
}

#[cfg(test)]
mod tests;
