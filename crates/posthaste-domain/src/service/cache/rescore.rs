use super::helpers::{
    body_fetch_bytes_from_metadata, body_fetch_unit, estimated_body_bytes_from_metadata,
    rescore_candidate_signals,
};
use super::*;

impl MailService {
    /// Re-score dirty cache candidates after local utility signals change.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    pub fn process_cache_rescore_batch(
        &self,
        account_id: &AccountId,
        batch_size: usize,
    ) -> Result<CacheRescoreBatchOutcome, ServiceError> {
        let mut outcome = CacheRescoreBatchOutcome::default();
        if batch_size == 0 {
            return Ok(outcome);
        }

        let candidates = self
            .cache_store
            .list_cache_rescore_candidates(account_id, batch_size)?;
        outcome.scanned = candidates.len();
        if candidates.is_empty() {
            ph_trace!(
                events::CACHE_RESCORE_NO_CANDIDATES,
                account_id = %account_id,
                "cache rescore worker found no dirty candidates"
            );
            return Ok(outcome);
        }

        let account = self.config.get_source(account_id)?;
        if account.is_none()
            && candidates
                .iter()
                .any(|candidate| candidate.layer == CacheLayer::Body)
        {
            return Err(StoreError::NotFound(format!("source:{}", account_id.as_str())).into());
        }
        let updates = candidates
            .iter()
            .map(|candidate| {
                let (fetch_unit, value_bytes, fetch_bytes) = match (&account, candidate.layer) {
                    (Some(account), CacheLayer::Body) => {
                        let fetch_unit = body_fetch_unit(account);
                        (
                            fetch_unit,
                            estimated_body_bytes_from_metadata(
                                candidate.message_size,
                                candidate.has_attachment,
                            ),
                            body_fetch_bytes_from_metadata(
                                account,
                                candidate.message_size,
                                candidate.has_attachment,
                            ),
                        )
                    }
                    _ => (
                        candidate.fetch_unit,
                        candidate.value_bytes,
                        candidate.fetch_bytes,
                    ),
                };
                let signals =
                    rescore_candidate_signals(candidate, fetch_unit, value_bytes, fetch_bytes);
                let score = score_cache_candidate(&signals);
                ph_trace!(
                    events::CACHE_RESCORE_CANDIDATE_SCORED,
                    account_id = %account_id,
                    message_id = candidate.message_id.as_str(),
                    layer = candidate.layer.as_str(),
                    fetch_unit = fetch_unit.as_str(),
                    value_bytes,
                    fetch_bytes,
                    old_priority = candidate.priority,
                    new_priority = score.priority,
                    utility = score.utility,
                    size_cost = score.size_cost,
                    signal_reason = candidate.signal_reason.as_str(),
                    rescore_priority = candidate.rescore_priority,
                    direct_user_boost = candidate.direct_user_boost,
                    search_result_rank = candidate.search.as_ref().map(|search| search.result_rank),
                    "cache candidate re-scored"
                );
                CachePriorityUpdate {
                    account_id: candidate.account_id.clone(),
                    message_id: candidate.message_id.clone(),
                    layer: candidate.layer,
                    object_id: candidate.object_id.clone(),
                    fetch_unit,
                    value_bytes,
                    fetch_bytes,
                    priority: score.priority,
                    reason: candidate.signal_reason.clone(),
                }
            })
            .collect::<Vec<_>>();
        self.cache_store.update_cache_priorities(&updates)?;
        outcome.updated = updates.len();
        ph_debug!(
            events::CACHE_RESCORE_COMPLETED,
            account_id = %account_id,
            scanned = outcome.scanned,
            updated = outcome.updated,
            "cache rescore worker batch completed"
        );
        Ok(outcome)
    }

    /// Queue stale cache objects for re-scoring so time-sensitive utility, such
    /// as recency, converges even without new sync or search signals.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    pub fn queue_stale_cache_rescore_batch(
        &self,
        account_id: &AccountId,
        stale_after: Duration,
        batch_size: usize,
    ) -> Result<usize, ServiceError> {
        if batch_size == 0 {
            return Ok(0);
        }
        let stale_seconds = i64::try_from(stale_after.as_secs()).unwrap_or(i64::MAX);
        let stale_before = (time::OffsetDateTime::now_utc()
            - time::Duration::seconds(stale_seconds))
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|err| StoreError::Failure(err.to_string()))?;
        let queued = self.cache_store.queue_stale_cache_rescore_candidates(
            account_id,
            stale_before.as_str(),
            batch_size,
        )?;
        if queued > 0 {
            ph_debug!(
                events::CACHE_RESCORE_STALE_QUEUED,
                account_id = %account_id,
                stale_after_seconds = stale_after.as_secs(),
                stale_before = stale_before.as_str(),
                queued,
                "stale cache candidates queued for re-score"
            );
        }
        Ok(queued)
    }
}
