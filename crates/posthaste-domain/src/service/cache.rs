use std::collections::HashSet;
use std::time::Duration;

use posthaste_observability::{events, ph_debug, ph_trace};

use crate::{
    decide_cache_admission, score_cache_candidate, AccountDriver, AccountId, AccountSettings,
    CacheAdmission, CacheCandidate, CacheCandidateSignals, CacheFetchLease, CacheFetchUnit,
    CacheLayer, CacheMessageSignals, CacheObjectState, CachePolicy, CachePriorityUpdate,
    CacheRescoreBatchOutcome, CacheRescoreCandidate, CacheSignalUpdate, CacheWorkerBatchOutcome,
    MailGateway, MessageId, MessagePage, MessageRecord, ServiceError, StoreError,
};

use super::MailService;

fn message_age_days(received_at: &str) -> f64 {
    let Ok(received_at) =
        time::OffsetDateTime::parse(received_at, &time::format_description::well_known::Rfc3339)
    else {
        return 365.0;
    };
    let now = time::OffsetDateTime::now_utc();
    let seconds = (now - received_at).whole_seconds().max(0) as f64;
    seconds / 86_400.0
}

fn nonnegative_message_size(size: i64) -> u64 {
    u64::try_from(size.max(0)).unwrap_or(0)
}

fn estimated_body_bytes(message: &MessageRecord) -> u64 {
    estimated_body_bytes_from_metadata(message.size, message.has_attachment)
}

fn estimated_body_bytes_from_metadata(size: i64, has_attachment: bool) -> u64 {
    let metadata_size = nonnegative_message_size(size);
    if metadata_size == 0 {
        return 64 * 1024;
    }
    if has_attachment {
        metadata_size.clamp(16 * 1024, 256 * 1024)
    } else {
        metadata_size.max(4 * 1024)
    }
}

fn body_fetch_unit(account: &AccountSettings) -> CacheFetchUnit {
    match account.driver {
        AccountDriver::ImapSmtp => CacheFetchUnit::RawMessage,
        AccountDriver::Jmap | AccountDriver::Mock => CacheFetchUnit::BodyOnly,
    }
}

fn body_fetch_bytes(account: &AccountSettings, message: &MessageRecord) -> u64 {
    body_fetch_bytes_from_metadata(account, message.size, message.has_attachment)
}

fn body_fetch_bytes_from_metadata(
    account: &AccountSettings,
    size: i64,
    has_attachment: bool,
) -> u64 {
    match body_fetch_unit(account) {
        CacheFetchUnit::RawMessage => nonnegative_message_size(size).max(4 * 1024),
        CacheFetchUnit::BodyOnly => estimated_body_bytes_from_metadata(size, has_attachment),
        CacheFetchUnit::AttachmentBlob => unreachable!("body cache never fetches attachment blobs"),
    }
}

fn visible_rank_direct_boost(rank: u64) -> f64 {
    0.8 / ((rank + 1) as f64).sqrt()
}

fn rescore_candidate_signals(
    candidate: &CacheRescoreCandidate,
    fetch_unit: CacheFetchUnit,
    value_bytes: u64,
    fetch_bytes: u64,
) -> CacheCandidateSignals {
    CacheCandidateSignals {
        message: CacheMessageSignals {
            age_days: message_age_days(&candidate.received_at),
            in_inbox: candidate.in_inbox,
            unread: candidate.unread,
            flagged: candidate.flagged,
            thread_activity: candidate.thread_activity,
            sender_affinity: candidate.sender_affinity,
            local_behavior: candidate.local_behavior,
            search: candidate.search.clone(),
        },
        layer: candidate.layer,
        fetch_unit,
        value_bytes,
        fetch_bytes,
        inline_attachment: false,
        opened_attachment: false,
        direct_user_boost: candidate.direct_user_boost,
        pinned: candidate.pinned,
    }
}

impl MailService {
    /// Record visible search results as cache utility signals.
    ///
    /// This only updates local signal state and the re-score queue; the account
    /// runtime performs remote fetches asynchronously.
    ///
    /// @spec docs/L1-sync#local-cache-planning
    pub fn record_cache_search_visibility(
        &self,
        page: &MessagePage,
        total_messages: u64,
        result_count: u64,
    ) -> Result<Vec<AccountId>, ServiceError> {
        if page.items.is_empty() {
            return Ok(Vec::new());
        }
        let total_messages = total_messages
            .max(result_count)
            .max(page.items.len() as u64);
        let result_count = result_count.max(page.items.len() as u64);
        let updates = page
            .items
            .iter()
            .enumerate()
            .map(|(rank, message)| CacheSignalUpdate {
                account_id: message.source_id.to_string(),
                message_id: message.id.to_string(),
                reason: "search-visible".to_string(),
                search: Some(crate::CacheSearchSignals {
                    total_messages,
                    result_count,
                    result_rank: rank as u64,
                }),
                thread_activity: None,
                sender_affinity: None,
                local_behavior: None,
                direct_user_boost: Some(visible_rank_direct_boost(rank as u64)),
                pinned: None,
            })
            .collect::<Vec<_>>();
        self.cache_store.record_cache_signal_updates(&updates)?;
        let account_ids = updates
            .iter()
            .map(|update| AccountId::from(update.account_id.as_str()))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        ph_debug!(
            events::CACHE_SEARCH_VISIBILITY_RECORDED,
            message_count = updates.len(),
            account_count = account_ids.len(),
            total_messages,
            result_count,
            "cache search visibility signals recorded"
        );
        Ok(account_ids)
    }

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

            let result =
                match self
                    .sync_writer
                    .apply_message_body(account_id, &message_id, &fetched)
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

    pub(super) fn upsert_body_cache_candidates(
        &self,
        account_id: &AccountId,
        account: &AccountSettings,
        policy: &CachePolicy,
        messages: &[MessageRecord],
    ) -> Result<(), ServiceError> {
        if !policy.cache_bodies || messages.is_empty() {
            ph_debug!(
                events::CACHE_BODY_CANDIDATE_GENERATION_SKIPPED,
                account_id = %account_id,
                message_count = messages.len(),
                cache_bodies = policy.cache_bodies,
                "cache candidate generation skipped"
            );
            return Ok(());
        }

        let inbox_mailbox_ids = self
            .mailbox_reader
            .list_mailboxes(account_id)?
            .into_iter()
            .filter(|mailbox| mailbox.role.as_deref() == Some("inbox"))
            .map(|mailbox| mailbox.id)
            .collect::<HashSet<_>>();
        let fetch_unit = body_fetch_unit(account);
        let candidates = messages
            .iter()
            .filter(|message| message.body_html.is_none() && message.body_text.is_none())
            .map(|message| {
                let value_bytes = estimated_body_bytes(message);
                let fetch_bytes = body_fetch_bytes(account, message);
                let signals = CacheCandidateSignals {
                    message: CacheMessageSignals {
                        age_days: message_age_days(&message.received_at),
                        in_inbox: message
                            .mailbox_ids
                            .iter()
                            .any(|mailbox_id| inbox_mailbox_ids.contains(mailbox_id)),
                        unread: !message.keywords.iter().any(|keyword| keyword == "$seen"),
                        flagged: message.keywords.iter().any(|keyword| keyword == "$flagged"),
                        thread_activity: 0.0,
                        sender_affinity: 0.0,
                        local_behavior: 0.0,
                        search: None,
                    },
                    layer: CacheLayer::Body,
                    fetch_unit,
                    value_bytes,
                    fetch_bytes,
                    inline_attachment: false,
                    opened_attachment: false,
                    direct_user_boost: 0.0,
                    pinned: false,
                };
                let score = score_cache_candidate(&signals);
                ph_trace!(
                    events::CACHE_BODY_CANDIDATE_SCORED,
                    account_id = %account_id,
                    message_id = %message.id,
                    layer = CacheLayer::Body.as_str(),
                    fetch_unit = fetch_unit.as_str(),
                    value_bytes,
                    fetch_bytes,
                    utility = score.utility,
                    size_cost = score.size_cost,
                    priority = score.priority,
                    age_days = signals.message.age_days,
                    in_inbox = signals.message.in_inbox,
                    unread = signals.message.unread,
                    flagged = signals.message.flagged,
                    "cache body candidate scored"
                );
                CacheCandidate {
                    account_id: account_id.to_string(),
                    message_id: message.id.to_string(),
                    layer: CacheLayer::Body,
                    object_id: None,
                    fetch_unit,
                    value_bytes,
                    fetch_bytes,
                    priority: score.priority,
                    reason: match fetch_unit {
                        CacheFetchUnit::BodyOnly => "body".to_string(),
                        CacheFetchUnit::RawMessage => "body-via-raw-message".to_string(),
                        CacheFetchUnit::AttachmentBlob => "body".to_string(),
                    },
                }
            })
            .collect::<Vec<_>>();
        let total_fetch_bytes = candidates
            .iter()
            .map(|candidate| candidate.fetch_bytes)
            .sum::<u64>();
        let total_value_bytes = candidates
            .iter()
            .map(|candidate| candidate.value_bytes)
            .sum::<u64>();
        ph_debug!(
            events::CACHE_BODY_CANDIDATES_SCORED,
            account_id = %account_id,
            driver = ?account.driver,
            fetch_unit = fetch_unit.as_str(),
            synced_message_count = messages.len(),
            candidate_count = candidates.len(),
            total_value_bytes,
            total_fetch_bytes,
            "cache body candidates scored"
        );
        self.cache_store.upsert_cache_candidates(&candidates)?;
        ph_debug!(
            events::CACHE_BODY_CANDIDATES_UPSERTED,
            account_id = %account_id,
            candidate_count = candidates.len(),
            "cache body candidates upserted"
        );
        Ok(())
    }
}
