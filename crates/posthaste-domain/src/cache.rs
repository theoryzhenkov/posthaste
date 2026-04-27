use serde::{Deserialize, Serialize};

const MIB: f64 = 1024.0 * 1024.0;
const MIN_BILLABLE_BYTES: f64 = 4.0 * 1024.0;
const DEFAULT_SIZE_ALPHA: f64 = 0.7;
const PINNED_BONUS: f64 = 4.0;

/// Tunable manual weights for optional local cache utility scoring.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheScoringWeights {
    pub recency: f64,
    pub thread_activity: f64,
    pub sender_affinity: f64,
    pub explicit_importance: f64,
    pub search_context: f64,
    pub local_behavior: f64,
    pub size_alpha: f64,
}

impl Default for CacheScoringWeights {
    fn default() -> Self {
        Self {
            recency: 0.35,
            thread_activity: 0.20,
            sender_affinity: 0.15,
            explicit_importance: 0.10,
            search_context: 0.10,
            local_behavior: 0.10,
            size_alpha: DEFAULT_SIZE_ALPHA,
        }
    }
}

/// Optional cache layer scored independently for the same message.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CacheLayer {
    Body,
    RawMessage,
    AttachmentBlob,
}

impl CacheLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Body => "body",
            Self::RawMessage => "raw_message",
            Self::AttachmentBlob => "attachment_blob",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "body" => Some(Self::Body),
            "raw_message" => Some(Self::RawMessage),
            "attachment_blob" => Some(Self::AttachmentBlob),
            _ => None,
        }
    }
}

/// Download/storage unit needed to satisfy a cache candidate.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CacheFetchUnit {
    BodyOnly,
    RawMessage,
    AttachmentBlob,
}

impl CacheFetchUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BodyOnly => "body_only",
            Self::RawMessage => "raw_message",
            Self::AttachmentBlob => "attachment_blob",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "body_only" => Some(Self::BodyOnly),
            "raw_message" => Some(Self::RawMessage),
            "attachment_blob" => Some(Self::AttachmentBlob),
            _ => None,
        }
    }
}

/// Persisted state for a cache candidate/fetch object.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CacheObjectState {
    Wanted,
    Fetching,
    Cached,
    Failed,
    Evicted,
}

impl CacheObjectState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wanted => "wanted",
            Self::Fetching => "fetching",
            Self::Cached => "cached",
            Self::Failed => "failed",
            Self::Evicted => "evicted",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "wanted" => Some(Self::Wanted),
            "fetching" => Some(Self::Fetching),
            "cached" => Some(Self::Cached),
            "failed" => Some(Self::Failed),
            "evicted" => Some(Self::Evicted),
            _ => None,
        }
    }
}

/// Global cache budget and layer eligibility.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePolicy {
    pub soft_cap_bytes: u64,
    pub hard_cap_bytes: u64,
    pub cache_bodies: bool,
    pub cache_raw_messages: bool,
    pub cache_attachments: bool,
}

impl Default for CachePolicy {
    fn default() -> Self {
        Self {
            soft_cap_bytes: 1024 * 1024 * 1024,
            hard_cap_bytes: 2 * 1024 * 1024 * 1024,
            cache_bodies: true,
            cache_raw_messages: false,
            cache_attachments: false,
        }
    }
}

impl CachePolicy {
    pub fn budget(self, used_bytes: u64, interactive_pressure: f64) -> CacheBudget {
        CacheBudget {
            used_bytes,
            soft_cap_bytes: self.soft_cap_bytes,
            hard_cap_bytes: self.hard_cap_bytes.max(self.soft_cap_bytes),
            interactive_pressure,
        }
    }
}

/// Search context that temporarily raises utility for visible, tight results.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheSearchSignals {
    pub total_messages: u64,
    pub result_count: u64,
    pub result_rank: u64,
}

/// Message-level signals used by the manual cache utility scorer.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheMessageSignals {
    pub age_days: f64,
    pub in_inbox: bool,
    pub unread: bool,
    pub flagged: bool,
    pub thread_activity: f64,
    pub sender_affinity: f64,
    pub local_behavior: f64,
    pub search: Option<CacheSearchSignals>,
}

/// Candidate-specific signals for one cacheable object.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheCandidateSignals {
    pub message: CacheMessageSignals,
    pub layer: CacheLayer,
    pub fetch_unit: CacheFetchUnit,
    pub value_bytes: u64,
    pub fetch_bytes: u64,
    pub inline_attachment: bool,
    pub opened_attachment: bool,
    pub direct_user_boost: f64,
    pub pinned: bool,
}

/// Scored cache candidate values before admission or eviction.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheScore {
    pub utility: f64,
    pub size_cost: f64,
    pub priority: f64,
}

/// Durable cache ledger row.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheObject {
    pub account_id: String,
    pub message_id: String,
    pub layer: CacheLayer,
    pub object_id: Option<String>,
    pub fetch_unit: CacheFetchUnit,
    pub state: CacheObjectState,
    pub value_bytes: u64,
    pub fetch_bytes: u64,
    pub priority: f64,
    pub reason: String,
    pub last_scored_at: String,
    pub last_accessed_at: Option<String>,
    pub fetched_at: Option<String>,
    pub error_code: Option<String>,
}

/// Upsert payload for a cache candidate.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheCandidate {
    pub account_id: String,
    pub message_id: String,
    pub layer: CacheLayer,
    pub object_id: Option<String>,
    pub fetch_unit: CacheFetchUnit,
    pub value_bytes: u64,
    pub fetch_bytes: u64,
    pub priority: f64,
    pub reason: String,
}

/// Candidate selected by the cache worker for a fetch attempt.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheFetchCandidate {
    pub account_id: String,
    pub message_id: String,
    pub layer: CacheLayer,
    pub object_id: Option<String>,
    pub fetch_unit: CacheFetchUnit,
    pub fetch_bytes: u64,
    pub priority: f64,
}

/// Message-level cache signal update from local user/app activity.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheSignalUpdate {
    pub account_id: String,
    pub message_id: String,
    pub reason: String,
    pub search: Option<CacheSearchSignals>,
    pub thread_activity: Option<f64>,
    pub sender_affinity: Option<f64>,
    pub local_behavior: Option<f64>,
    pub direct_user_boost: Option<f64>,
    pub pinned: Option<bool>,
}

/// Cache object plus current metadata/signals used by the re-score worker.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheRescoreCandidate {
    pub account_id: String,
    pub message_id: String,
    pub layer: CacheLayer,
    pub object_id: Option<String>,
    pub fetch_unit: CacheFetchUnit,
    pub state: CacheObjectState,
    pub value_bytes: u64,
    pub fetch_bytes: u64,
    pub priority: f64,
    pub message_size: i64,
    pub has_attachment: bool,
    pub received_at: String,
    pub in_inbox: bool,
    pub unread: bool,
    pub flagged: bool,
    pub thread_activity: f64,
    pub sender_affinity: f64,
    pub local_behavior: f64,
    pub search: Option<CacheSearchSignals>,
    pub direct_user_boost: f64,
    pub pinned: bool,
    pub signal_reason: String,
}

/// Priority update emitted by the re-score worker.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachePriorityUpdate {
    pub account_id: String,
    pub message_id: String,
    pub layer: CacheLayer,
    pub object_id: Option<String>,
    pub fetch_unit: CacheFetchUnit,
    pub value_bytes: u64,
    pub fetch_bytes: u64,
    pub priority: f64,
    pub reason: String,
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

/// Current cache budget and pressure state for admission decisions.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheBudget {
    pub used_bytes: u64,
    pub soft_cap_bytes: u64,
    pub hard_cap_bytes: u64,
    pub interactive_pressure: f64,
}

impl CacheBudget {
    /// Soft cap plus a bounded fraction of the burst space toward the hard cap.
    pub fn effective_target_bytes(self) -> u64 {
        let hard = self.hard_cap_bytes.max(self.soft_cap_bytes);
        let pressure = clamp_unit(self.interactive_pressure);
        let burst_range = hard.saturating_sub(self.soft_cap_bytes) as f64;
        self.soft_cap_bytes + (burst_range * pressure).round() as u64
    }
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

fn clamp_unit(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_message() -> CacheMessageSignals {
        CacheMessageSignals {
            age_days: 2.0,
            in_inbox: true,
            unread: true,
            flagged: false,
            thread_activity: 0.0,
            sender_affinity: 0.0,
            local_behavior: 0.0,
            search: None,
        }
    }

    // spec: docs/L1-sync#cache-priority-size-aware
    #[test]
    fn recent_unread_body_scores_above_old_unread_body() {
        let mut old = base_message();
        old.age_days = 365.0;

        let recent_score = score_cache_candidate(&CacheCandidateSignals {
            message: base_message(),
            layer: CacheLayer::Body,
            fetch_unit: CacheFetchUnit::BodyOnly,
            value_bytes: 64 * 1024,
            fetch_bytes: 64 * 1024,
            inline_attachment: false,
            opened_attachment: false,
            direct_user_boost: 0.0,
            pinned: false,
        });
        let old_score = score_cache_candidate(&CacheCandidateSignals {
            message: old,
            layer: CacheLayer::Body,
            fetch_unit: CacheFetchUnit::BodyOnly,
            value_bytes: 64 * 1024,
            fetch_bytes: 64 * 1024,
            inline_attachment: false,
            opened_attachment: false,
            direct_user_boost: 0.0,
            pinned: false,
        });

        assert!(recent_score.priority > old_score.priority);
    }

    // spec: docs/L1-sync#cache-priority-size-aware
    #[test]
    fn high_value_large_attachment_can_beat_low_value_small_attachment() {
        let mut high_value = base_message();
        high_value.flagged = true;
        high_value.thread_activity = 5.0;
        high_value.sender_affinity = 5.0;

        let mut low_value = base_message();
        low_value.age_days = 180.0;
        low_value.in_inbox = false;
        low_value.unread = false;

        let large_score = score_cache_candidate(&CacheCandidateSignals {
            message: high_value,
            layer: CacheLayer::AttachmentBlob,
            fetch_unit: CacheFetchUnit::AttachmentBlob,
            value_bytes: 20 * 1024 * 1024,
            fetch_bytes: 20 * 1024 * 1024,
            inline_attachment: false,
            opened_attachment: true,
            direct_user_boost: 0.0,
            pinned: false,
        });
        let small_score = score_cache_candidate(&CacheCandidateSignals {
            message: low_value,
            layer: CacheLayer::AttachmentBlob,
            fetch_unit: CacheFetchUnit::AttachmentBlob,
            value_bytes: 1024 * 1024,
            fetch_bytes: 1024 * 1024,
            inline_attachment: false,
            opened_attachment: false,
            direct_user_boost: 0.0,
            pinned: false,
        });

        assert!(large_score.priority > small_score.priority);
    }

    // spec: docs/L1-sync#cache-priority-size-aware
    #[test]
    fn tight_visible_search_result_boosts_priority() {
        let mut searched = base_message();
        searched.search = Some(CacheSearchSignals {
            total_messages: 100_000,
            result_count: 12,
            result_rank: 0,
        });

        let searched_score = score_cache_candidate(&CacheCandidateSignals {
            message: searched,
            layer: CacheLayer::Body,
            fetch_unit: CacheFetchUnit::BodyOnly,
            value_bytes: 64 * 1024,
            fetch_bytes: 64 * 1024,
            inline_attachment: false,
            opened_attachment: false,
            direct_user_boost: 0.0,
            pinned: false,
        });
        let baseline_score = score_cache_candidate(&CacheCandidateSignals {
            message: base_message(),
            layer: CacheLayer::Body,
            fetch_unit: CacheFetchUnit::BodyOnly,
            value_bytes: 64 * 1024,
            fetch_bytes: 64 * 1024,
            inline_attachment: false,
            opened_attachment: false,
            direct_user_boost: 0.0,
            pinned: false,
        });

        assert!(searched_score.priority > baseline_score.priority);
    }

    // spec: docs/L1-sync#cache-priority-size-aware
    #[test]
    fn body_priority_uses_fetch_unit_cost_not_body_value_size() {
        let jmap_score = score_cache_candidate(&CacheCandidateSignals {
            message: base_message(),
            layer: CacheLayer::Body,
            fetch_unit: CacheFetchUnit::BodyOnly,
            value_bytes: 64 * 1024,
            fetch_bytes: 64 * 1024,
            inline_attachment: false,
            opened_attachment: false,
            direct_user_boost: 0.0,
            pinned: false,
        });
        let imap_score = score_cache_candidate(&CacheCandidateSignals {
            message: base_message(),
            layer: CacheLayer::Body,
            fetch_unit: CacheFetchUnit::RawMessage,
            value_bytes: 64 * 1024,
            fetch_bytes: 12 * 1024 * 1024,
            inline_attachment: false,
            opened_attachment: false,
            direct_user_boost: 0.0,
            pinned: false,
        });

        assert!(jmap_score.priority > imap_score.priority);
    }

    // spec: docs/L1-sync#cache-admission-hard-cap
    #[test]
    fn interactive_pressure_raises_target_between_soft_and_hard_caps() {
        let limits = CacheBudget {
            used_bytes: 900,
            soft_cap_bytes: 1_000,
            hard_cap_bytes: 2_000,
            interactive_pressure: 0.75,
        };

        assert_eq!(limits.effective_target_bytes(), 1_750);
    }

    // spec: docs/L1-sync#cache-admission-hard-cap
    #[test]
    fn admission_allows_soft_cap_burst_but_never_crosses_hard_cap() {
        let limits = CacheBudget {
            used_bytes: 1_600,
            soft_cap_bytes: 1_000,
            hard_cap_bytes: 2_000,
            interactive_pressure: 0.75,
        };

        assert_eq!(
            decide_cache_admission(100, 2.0, Some(1.0), &limits),
            CacheAdmission::AdmitWithinTarget
        );
        assert_eq!(
            decide_cache_admission(500, 10.0, Some(1.0), &limits),
            CacheAdmission::RejectOverHardCap
        );
    }

    // spec: docs/L1-sync#cache-priority-size-aware
    #[test]
    fn admission_requires_beating_evictable_priority_when_over_target() {
        let limits = CacheBudget {
            used_bytes: 1_900,
            soft_cap_bytes: 1_000,
            hard_cap_bytes: 3_000,
            interactive_pressure: 0.25,
        };

        assert_eq!(
            decide_cache_admission(100, 0.5, Some(1.0), &limits),
            CacheAdmission::RejectLowerPriority
        );
        assert_eq!(
            decide_cache_admission(100, 1.5, Some(1.0), &limits),
            CacheAdmission::AdmitByReplacingLowerPriority
        );
    }
}
