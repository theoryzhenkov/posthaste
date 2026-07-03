use super::*;
use crate::service::offload;

impl MailService {
    /// Fetch one bounded batch of wanted message-body cache candidates.
    ///
    /// The first worker slice has no eviction path, so it admits only bodies
    /// that fit under the current effective background target.
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

            let fetched = match gateway.fetch_message_body(account_id, &message_id).await {
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
                sync_writer.apply_message_body(&owned_account_id, &owned_message_id, &fetched)
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
