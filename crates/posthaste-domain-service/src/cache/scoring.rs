use super::*;

/// Cheap urgency estimate for ordering dirty cache objects before full scoring.
///
/// This is deliberately separate from final fetch priority: it only decides
/// which dirty objects should be re-scored first. Final cache priority is still
/// computed from full candidate metadata after the row leaves this queue.
///
/// @spec docs/L1-sync#cache-signal-rescore
pub fn cache_signal_rescore_priority(update: &CacheSignalUpdate) -> f64 {
    let search = update
        .search
        .as_ref()
        .map(search_context_score)
        .unwrap_or(0.0);
    let direct_user_boost = finite_nonnegative(update.direct_user_boost.unwrap_or(0.0));
    let thread_activity = saturating_signal(update.thread_activity.unwrap_or(0.0), 4.0);
    let sender_affinity = saturating_signal(update.sender_affinity.unwrap_or(0.0), 4.0);
    let local_behavior = saturating_signal(update.local_behavior.unwrap_or(0.0), 4.0);
    let pinned = if update.pinned.unwrap_or(false) {
        PINNED_BONUS
    } else {
        0.0
    };

    let signal_urgency = 10.0 * direct_user_boost
        + 8.0 * search
        + 4.0 * thread_activity
        + 2.0 * sender_affinity
        + 2.0 * local_behavior
        + pinned;

    if signal_urgency > 0.0 {
        LOCAL_SIGNAL_RESCORE_BASE + signal_urgency
    } else {
        1.0
    }
}

/// Result of checking a candidate against the current cache budget.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CacheAdmission {
    AdmitWithinTarget,
    AdmitByReplacingLowerPriority,
    RejectLowerPriority,
    RejectNoEvictableCandidate,
    RejectOverHardCap,
}

/// Score a cache candidate with the default manual weights.
///
/// @spec docs/L1-sync#cache-priority-size-aware
pub fn score_cache_candidate(candidate: &CacheCandidateSignals) -> CacheScore {
    score_cache_candidate_with_weights(candidate, &CacheScoringWeights::default())
}

/// Score a cache candidate with explicit manual weights.
///
/// @spec docs/L1-sync#cache-priority-size-aware
pub fn score_cache_candidate_with_weights(
    candidate: &CacheCandidateSignals,
    weights: &CacheScoringWeights,
) -> CacheScore {
    let message_utility = message_utility(&candidate.message, weights);
    let layer_weight = layer_weight(candidate.layer);
    let object_modifier = object_modifier(candidate);
    let direct_user_boost = finite_nonnegative(candidate.direct_user_boost);
    let pin_bonus = if candidate.pinned { PINNED_BONUS } else { 0.0 };
    let utility =
        (message_utility * layer_weight * object_modifier) + direct_user_boost + pin_bonus;
    let size_cost = size_cost(candidate.fetch_bytes, weights.size_alpha);
    CacheScore {
        utility,
        size_cost,
        priority: utility / size_cost,
    }
}

/// Decide if a candidate can be admitted under the effective and hard caps.
///
/// @spec docs/L1-sync#cache-admission-hard-cap
pub fn decide_cache_admission(
    candidate_size_bytes: u64,
    candidate_priority: f64,
    lowest_evictable_cached_priority: Option<f64>,
    budget: &CacheBudget,
) -> CacheAdmission {
    if budget.used_bytes.saturating_add(candidate_size_bytes) > budget.hard_cap_bytes {
        return CacheAdmission::RejectOverHardCap;
    }

    if budget.used_bytes.saturating_add(candidate_size_bytes) <= budget.effective_target_bytes() {
        return CacheAdmission::AdmitWithinTarget;
    }

    if !candidate_priority.is_finite() || candidate_priority < 0.0 {
        return CacheAdmission::RejectLowerPriority;
    }

    let Some(lowest_priority) = lowest_evictable_cached_priority else {
        return CacheAdmission::RejectNoEvictableCandidate;
    };
    if lowest_priority.is_finite() && candidate_priority > lowest_priority.max(0.0) {
        CacheAdmission::AdmitByReplacingLowerPriority
    } else {
        CacheAdmission::RejectLowerPriority
    }
}

fn message_utility(message: &CacheMessageSignals, weights: &CacheScoringWeights) -> f64 {
    let recency = half_life_decay(message.age_days, 30.0);
    let thread_activity = saturating_signal(message.thread_activity, 4.0);
    let sender_affinity = saturating_signal(message.sender_affinity, 4.0);
    let explicit_importance = explicit_importance(message);
    let search_context = message
        .search
        .as_ref()
        .map(search_context_score)
        .unwrap_or(0.0);
    let local_behavior = saturating_signal(message.local_behavior, 4.0);

    weights.recency.max(0.0) * recency
        + weights.thread_activity.max(0.0) * thread_activity
        + weights.sender_affinity.max(0.0) * sender_affinity
        + weights.explicit_importance.max(0.0) * explicit_importance
        + weights.search_context.max(0.0) * search_context
        + weights.local_behavior.max(0.0) * local_behavior
}

fn explicit_importance(message: &CacheMessageSignals) -> f64 {
    if message.flagged {
        return 1.0;
    }
    match (message.unread, message.in_inbox) {
        (true, true) => 0.6,
        (true, false) => 0.4,
        (false, true) => 0.2,
        (false, false) => 0.0,
    }
}

fn search_context_score(search: &CacheSearchSignals) -> f64 {
    if search.total_messages == 0 || search.result_count == 0 {
        return 0.0;
    }
    let total = search.total_messages as f64 + 1.0;
    let result_count = search.result_count.min(search.total_messages) as f64 + 1.0;
    let tightness = 1.0 - (result_count.ln() / total.ln());
    let rank_decay = 1.0 / ((search.result_rank + 1) as f64).sqrt();
    clamp_unit(tightness) * rank_decay
}

fn layer_weight(layer: CacheLayer) -> f64 {
    match layer {
        CacheLayer::Body => 1.0,
        CacheLayer::RawMessage => 0.45,
        CacheLayer::AttachmentBlob => 0.25,
    }
}

fn object_modifier(candidate: &CacheCandidateSignals) -> f64 {
    match candidate.layer {
        CacheLayer::Body | CacheLayer::RawMessage => 1.0,
        CacheLayer::AttachmentBlob if candidate.opened_attachment => 3.0,
        CacheLayer::AttachmentBlob if candidate.inline_attachment => 1.6,
        CacheLayer::AttachmentBlob => 1.0,
    }
}

fn size_cost(size_bytes: u64, alpha: f64) -> f64 {
    let size_bytes = (size_bytes as f64).max(MIN_BILLABLE_BYTES);
    let alpha = finite_nonnegative(alpha).clamp(0.1, 2.0);
    (size_bytes / MIB).powf(alpha)
}

fn half_life_decay(age_days: f64, half_life_days: f64) -> f64 {
    2.0_f64.powf(-finite_nonnegative(age_days) / half_life_days.max(1.0))
}

fn saturating_signal(value: f64, saturation: f64) -> f64 {
    let value = finite_nonnegative(value);
    1.0 - (-value / saturation.max(1.0)).exp()
}

fn finite_nonnegative(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

pub(crate) fn lerp(start: f64, end: f64, amount: f64) -> f64 {
    start + (end - start) * clamp_unit(amount)
}
