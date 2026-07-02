use super::*;

/// Durable optional-content cache ledger boundary.
pub trait CacheStore: Send + Sync {
    /// Upsert scored cache candidates derived from synced metadata.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    fn upsert_cache_candidates(&self, candidates: &[CacheCandidate]) -> Result<(), StoreError>;

    /// Record local cache utility signals and enqueue affected messages for re-scoring.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    fn record_cache_signal_updates(&self, updates: &[CacheSignalUpdate]) -> Result<(), StoreError>;

    /// Return dirty cache objects with metadata needed for priority re-scoring.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    fn list_cache_rescore_candidates(
        &self,
        account_id: &AccountId,
        limit: usize,
    ) -> Result<Vec<CacheRescoreCandidate>, StoreError>;

    /// Queue stale cache objects for re-scoring in bounded oldest-first batches.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    fn queue_stale_cache_rescore_candidates(
        &self,
        account_id: &AccountId,
        stale_before: &str,
        limit: usize,
    ) -> Result<usize, StoreError>;

    /// Persist re-scored priorities and clear the corresponding dirty queue rows.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    fn update_cache_priorities(&self, updates: &[CachePriorityUpdate]) -> Result<(), StoreError>;

    /// Return highest-priority fetch candidates for an account/layer.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    fn list_cache_fetch_candidates(
        &self,
        account_id: &AccountId,
        layer: CacheLayer,
        limit: usize,
    ) -> Result<Vec<CacheFetchCandidate>, StoreError>;

    /// Mark a candidate state transition in the cache ledger.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    fn mark_cache_object_state(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        layer: CacheLayer,
        object_id: Option<&str>,
        state: CacheObjectState,
        error_code: Option<&str>,
    ) -> Result<(), StoreError>;

    /// Sum cached optional-content bytes for budget decisions.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    fn cache_used_bytes(&self) -> Result<u64, StoreError>;
}
