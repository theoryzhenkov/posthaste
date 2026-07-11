use super::*;
use crate::service::offload;
use posthaste_domain_model::BODY_CACHE_BATCH_BUDGET;

/// Error code recorded on a candidate whose in-flight fetch was cut short by
/// the batch-level deadline ([`BODY_CACHE_BATCH_BUDGET`]): the candidate is
/// marked `Failed` — never left stuck `Fetching`, which would leak it out of
/// the wanted set forever (Failed rows can be re-scored; Fetching rows are
/// excluded from both fetch and rescore candidate queries).
pub const BODY_CACHE_BATCH_DEADLINE_ERROR_CODE: &str = "batch_deadline";

impl MailService {
    /// Fetch one bounded batch of wanted message-body cache candidates.
    ///
    /// The first worker slice has no eviction path, so it admits only bodies
    /// that fit under the current effective background target.
    ///
    /// The whole batch runs under its own wall-clock deadline
    /// ([`BODY_CACHE_BATCH_BUDGET`]), checked before each candidate, and the
    /// in-flight provider fetch is bounded to the remaining budget. A slow or
    /// hung body source therefore makes the batch *return* with partial work
    /// (letting the caller record governor feedback and back off) instead of
    /// legitimately exceeding the supervisor's cache arm budget and being
    /// dropped mid-flight — the drop path never records feedback, so the 2 s
    /// cache tick would re-hit the slow server forever.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    pub async fn process_body_cache_batch(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
        lease: CacheFetchLease,
    ) -> Result<CacheWorkerBatchOutcome, ServiceError> {
        let mut outcome = CacheWorkerBatchOutcome::default();
        if !lease.has_fetch_budget() {
            return Ok(outcome);
        }
        // tokio's clock (not std's) so paused-time tests can drive the
        // deadline virtually; in production they are the same clock.
        let batch_started = tokio::time::Instant::now();

        let settings = self.config.get_app_settings()?;
        if !settings.cache_policy.cache_bodies {
            ph_debug!(
                events::CACHE_BODY_SKIPPED_DISABLED,
                account_id = %account_id,
                layer = CacheLayer::Body.as_str(),
                "cache worker skipped because body caching is disabled"
            );
            return Ok(outcome);
        }

        let mut used_bytes = self.cache_store.cache_used_bytes()?;
        let scan_limit = lease
            .request_limit
            .saturating_mul(4)
            .max(lease.request_limit);
        let initial_budget = settings
            .cache_policy
            .clone()
            .budget(used_bytes, lease.interactive_pressure);
        let candidates = self.cache_store.list_cache_fetch_candidates(
            account_id,
            CacheLayer::Body,
            scan_limit,
        )?;
        let candidate_count = candidates.len();
        if candidate_count > 0 {
            ph_debug!(
                events::CACHE_BODY_PLAN_CREATED,
                account_id = %account_id,
                layer = CacheLayer::Body.as_str(),
                request_limit = lease.request_limit,
                byte_limit = lease.byte_limit,
                scan_limit,
                candidate_count,
                used_bytes,
                soft_cap_bytes = initial_budget.soft_cap_bytes,
                effective_target_bytes = initial_budget.effective_target_bytes(),
                hard_cap_bytes = initial_budget.hard_cap_bytes,
                interactive_pressure = initial_budget.interactive_pressure,
                "cache worker body batch planned"
            );
        } else {
            ph_trace!(
                events::CACHE_BODY_PLAN_CREATED,
                account_id = %account_id,
                layer = CacheLayer::Body.as_str(),
                request_limit = lease.request_limit,
                byte_limit = lease.byte_limit,
                scan_limit,
                candidate_count,
                used_bytes,
                soft_cap_bytes = initial_budget.soft_cap_bytes,
                effective_target_bytes = initial_budget.effective_target_bytes(),
                hard_cap_bytes = initial_budget.hard_cap_bytes,
                interactive_pressure = initial_budget.interactive_pressure,
                "cache worker body batch planned"
            );
        }
        if candidates.is_empty() {
            ph_trace!(
                events::CACHE_BODY_NO_CANDIDATES,
                account_id = %account_id,
                layer = CacheLayer::Body.as_str(),
                "cache worker found no wanted body candidates"
            );
        }
        let mut remaining_lease_bytes = lease.byte_limit;
        for candidate in candidates {
            if outcome.attempted >= lease.request_limit {
                break;
            }
            // Batch deadline (see the method doc): stop cleanly with partial
            // work once the budget is spent — the remaining candidates stay
            // `wanted` for a later batch.
            if batch_started.elapsed() >= BODY_CACHE_BATCH_BUDGET {
                outcome.deadline_exceeded = true;
                ph_debug!(
                    events::CACHE_BODY_BATCH_DEADLINE,
                    account_id = %account_id,
                    layer = CacheLayer::Body.as_str(),
                    budget_ms = BODY_CACHE_BATCH_BUDGET.as_millis() as u64,
                    attempted = outcome.attempted,
                    cached = outcome.cached,
                    failed = outcome.failed,
                    "cache worker body batch stopped at its deadline with partial work"
                );
                break;
            }
            outcome.scanned += 1;
            if candidate.fetch_bytes > remaining_lease_bytes {
                outcome.skipped += 1;
                ph_trace!(
                    events::CACHE_BODY_DEFERRED_BY_LEASE,
                    account_id = %account_id,
                    message_id = candidate.message_id.as_str(),
                    layer = candidate.layer.as_str(),
                    fetch_unit = candidate.fetch_unit.as_str(),
                    fetch_bytes = candidate.fetch_bytes,
                    remaining_lease_bytes,
                    "cache candidate deferred by fetch byte lease"
                );
                continue;
            }
            let budget = settings
                .cache_policy
                .clone()
                .budget(used_bytes, lease.interactive_pressure);
            let admission =
                decide_cache_admission(candidate.fetch_bytes, candidate.priority, None, &budget);
            ph_trace!(
                events::CACHE_BODY_ADMISSION_EVALUATED,
                account_id = %account_id,
                message_id = candidate.message_id.as_str(),
                layer = candidate.layer.as_str(),
                fetch_unit = candidate.fetch_unit.as_str(),
                fetch_bytes = candidate.fetch_bytes,
                priority = candidate.priority,
                admission = ?admission,
                used_bytes = budget.used_bytes,
                effective_target_bytes = budget.effective_target_bytes(),
                hard_cap_bytes = budget.hard_cap_bytes,
                "cache candidate admission evaluated"
            );
            if admission != CacheAdmission::AdmitWithinTarget {
                outcome.skipped += 1;
                continue;
            }

            let message_id = MessageId::from(candidate.message_id.as_str());
            self.cache_store.mark_cache_object_state(
                account_id,
                &message_id,
                candidate.layer,
                candidate.object_id.as_deref(),
                CacheObjectState::Fetching,
                None,
            )?;
            outcome.attempted += 1;
            outcome.attempted_bytes = outcome
                .attempted_bytes
                .saturating_add(candidate.fetch_bytes);
            remaining_lease_bytes = remaining_lease_bytes.saturating_sub(candidate.fetch_bytes);
            ph_trace!(
                events::CACHE_BODY_FETCH_STARTED,
                account_id = %account_id,
                message_id = %message_id,
                layer = candidate.layer.as_str(),
                fetch_unit = candidate.fetch_unit.as_str(),
                fetch_bytes = candidate.fetch_bytes,
                priority = candidate.priority,
                "cache candidate fetch started"
            );

            // Bound the in-flight fetch to the batch's remaining budget: a
            // hung provider call is cut here (and the candidate marked Failed
            // below) rather than dragging the whole batch past the supervisor
            // arm budget. The per-call provider envelopes (IMAP 60 s per-op,
            // JMAP per-class deadline/stall) usually fire first; this is the
            // batch-shaped ceiling over them.
            let remaining_budget = BODY_CACHE_BATCH_BUDGET.saturating_sub(batch_started.elapsed());
            let fetch_result = match tokio::time::timeout(
                remaining_budget,
                gateway.fetch_message_body(account_id, &message_id),
            )
            .await
            {
                Ok(result) => result,
                Err(_elapsed) => {
                    // Cut short by the batch deadline: mark Failed (never
                    // leave it stuck Fetching) and stop the batch.
                    outcome.deadline_exceeded = true;
                    outcome.failed += 1;
                    ph_debug!(
                        events::CACHE_BODY_BATCH_DEADLINE,
                        account_id = %account_id,
                        message_id = %message_id,
                        layer = candidate.layer.as_str(),
                        fetch_unit = candidate.fetch_unit.as_str(),
                        budget_ms = BODY_CACHE_BATCH_BUDGET.as_millis() as u64,
                        "cache candidate fetch cut short by the batch deadline"
                    );
                    self.cache_store.mark_cache_object_state(
                        account_id,
                        &message_id,
                        candidate.layer,
                        candidate.object_id.as_deref(),
                        CacheObjectState::Failed,
                        Some(BODY_CACHE_BATCH_DEADLINE_ERROR_CODE),
                    )?;
                    break;
                }
            };
            let fetched = match fetch_result {
                Ok(fetched) => fetched,
                Err(error) => {
                    let service_error = ServiceError::from(error);
                    let error_code = service_error.code().to_string();
                    ph_debug!(
                        events::CACHE_BODY_FETCH_FAILED,
                        account_id = %account_id,
                        message_id = %message_id,
                        layer = candidate.layer.as_str(),
                        fetch_unit = candidate.fetch_unit.as_str(),
                        error_code = error_code.as_str(),
                        "cache candidate fetch failed"
                    );
                    self.cache_store.mark_cache_object_state(
                        account_id,
                        &message_id,
                        candidate.layer,
                        candidate.object_id.as_deref(),
                        CacheObjectState::Failed,
                        Some(error_code.as_str()),
                    )?;
                    outcome.failed += 1;
                    continue;
                }
            };

            let sync_writer = self.sync_writer.clone();
            let owned_account_id = account_id.clone();
            let owned_message_id = message_id.clone();
            let result = match offload(move || {
                sync_writer.apply_message_body(
                    &crate::BaseWrite::reconciler(),
                    &owned_account_id,
                    &owned_message_id,
                    &fetched,
                )
            })
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    let service_error = ServiceError::from(error);
                    let error_code = service_error.code().to_string();
                    self.cache_store.mark_cache_object_state(
                        account_id,
                        &message_id,
                        candidate.layer,
                        candidate.object_id.as_deref(),
                        CacheObjectState::Failed,
                        Some(error_code.as_str()),
                    )?;
                    return Err(service_error);
                }
            };
            self.cache_store.mark_cache_object_state(
                account_id,
                &message_id,
                candidate.layer,
                candidate.object_id.as_deref(),
                CacheObjectState::Cached,
                None,
            )?;
            used_bytes = used_bytes.saturating_add(candidate.fetch_bytes);
            outcome.cached += 1;
            outcome.cached_bytes = outcome.cached_bytes.saturating_add(candidate.fetch_bytes);
            outcome.events.extend(result.events);
            ph_trace!(
                events::CACHE_BODY_STORED,
                account_id = %account_id,
                message_id = %message_id,
                layer = candidate.layer.as_str(),
                fetch_unit = candidate.fetch_unit.as_str(),
                fetch_bytes = candidate.fetch_bytes,
                used_bytes,
                "cache candidate stored"
            );
        }

        Ok(outcome)
    }
}
