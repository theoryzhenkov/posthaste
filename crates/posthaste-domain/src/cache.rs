use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const MIB: f64 = 1024.0 * 1024.0;
const MIN_BILLABLE_BYTES: f64 = 4.0 * 1024.0;
const DEFAULT_SIZE_ALPHA: f64 = 0.7;
const PINNED_BONUS: f64 = 4.0;
const LOCAL_SIGNAL_RESCORE_BASE: f64 = 100.0;
const BYTES_PER_MIB: f64 = 1024.0 * 1024.0;

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

/// Resource policy for optional-content cache maintenance.
///
/// The priority queue decides which cache objects matter most. This policy
/// decides how much work a runtime may attempt before yielding to the app and
/// device.
///
/// @spec docs/L1-sync#cache-resource-governor
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CacheResourcePolicy {
    pub rescore_rows_per_second: f64,
    pub rescore_burst_rows: usize,
    pub max_rescore_rows_per_lease: usize,
    pub interactive_min_rescore_rows: usize,
    pub stale_rescore_fraction: f64,
    pub background_fetch_requests_per_second: f64,
    pub interactive_fetch_requests_per_second: f64,
    pub fetch_request_burst: usize,
    pub max_background_fetch_requests_per_lease: usize,
    pub max_interactive_fetch_requests_per_lease: usize,
    pub background_fetch_bytes_per_second: f64,
    pub interactive_fetch_bytes_per_second: f64,
    pub fetch_byte_burst: u64,
    pub max_background_fetch_bytes_per_lease: u64,
    pub max_interactive_fetch_bytes_per_lease: u64,
    pub interactive_min_fetch_bytes: u64,
    pub min_fetch_bytes_per_lease: u64,
    pub min_network_rate_multiplier: f64,
}

impl Default for CacheResourcePolicy {
    fn default() -> Self {
        Self {
            rescore_rows_per_second: 50.0,
            rescore_burst_rows: 500,
            max_rescore_rows_per_lease: 200,
            interactive_min_rescore_rows: 200,
            stale_rescore_fraction: 0.25,
            background_fetch_requests_per_second: 0.3,
            interactive_fetch_requests_per_second: 1.5,
            fetch_request_burst: 8,
            max_background_fetch_requests_per_lease: 2,
            max_interactive_fetch_requests_per_lease: 6,
            background_fetch_bytes_per_second: 256.0 * 1024.0,
            interactive_fetch_bytes_per_second: 2.0 * BYTES_PER_MIB,
            fetch_byte_burst: 64 * 1024 * 1024,
            max_background_fetch_bytes_per_lease: 4 * 1024 * 1024,
            max_interactive_fetch_bytes_per_lease: 32 * 1024 * 1024,
            interactive_min_fetch_bytes: 4 * 1024 * 1024,
            min_fetch_bytes_per_lease: 64 * 1024,
            min_network_rate_multiplier: 0.125,
        }
    }
}

/// Fetch-side lease granted to the cache worker.
///
/// @spec docs/L1-sync#cache-resource-governor
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CacheFetchLease {
    pub request_limit: usize,
    pub byte_limit: u64,
    pub interactive_pressure: f64,
}

impl CacheFetchLease {
    pub fn none(interactive_pressure: f64) -> Self {
        Self {
            request_limit: 0,
            byte_limit: 0,
            interactive_pressure: clamp_unit(interactive_pressure),
        }
    }

    pub fn new(request_limit: usize, byte_limit: u64, interactive_pressure: f64) -> Self {
        Self {
            request_limit,
            byte_limit,
            interactive_pressure: clamp_unit(interactive_pressure),
        }
    }

    pub fn has_fetch_budget(self) -> bool {
        self.request_limit > 0 && self.byte_limit > 0
    }
}

/// Cache-maintenance work allowance for one runtime slice.
///
/// @spec docs/L1-sync#cache-resource-governor
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CacheMaintenanceLease {
    pub stale_rescore_limit: usize,
    pub rescore_limit: usize,
    pub fetch: CacheFetchLease,
    pub network_rate_multiplier: f64,
    pub in_backoff: bool,
}

/// Observed work from one cache-maintenance slice.
///
/// @spec docs/L1-sync#cache-resource-governor
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CacheMaintenanceFeedback {
    pub stale_rescore_queued: usize,
    pub rescore_scanned: usize,
    pub fetch_attempted: usize,
    pub fetch_attempted_bytes: u64,
    pub fetch_cached: usize,
    pub fetch_failed: usize,
    pub elapsed: Duration,
    pub had_error: bool,
    pub had_fetch_error: bool,
}

/// Stateful token-bucket governor for optional-content cache maintenance.
///
/// Re-score tokens bound local CPU/SQLite work. Fetch request and byte tokens
/// bound provider/network pressure. Failures lower the network multiplier and
/// enter short backoff; successful fetches gradually restore the multiplier.
///
/// @spec docs/L1-sync#cache-resource-governor
#[derive(Clone, Debug)]
pub struct CacheResourceGovernor {
    policy: CacheResourcePolicy,
    rescore_tokens: f64,
    fetch_request_tokens: f64,
    fetch_byte_tokens: f64,
    last_refill: Instant,
    network_rate_multiplier: f64,
    consecutive_fetch_failures: u32,
    backoff_until: Option<Instant>,
}

impl CacheResourceGovernor {
    pub fn new(now: Instant, policy: CacheResourcePolicy) -> Self {
        Self {
            policy,
            rescore_tokens: policy.rescore_burst_rows as f64,
            fetch_request_tokens: policy.fetch_request_burst as f64,
            fetch_byte_tokens: policy.fetch_byte_burst as f64,
            last_refill: now,
            network_rate_multiplier: 1.0,
            consecutive_fetch_failures: 0,
            backoff_until: None,
        }
    }

    pub fn grant(&mut self, now: Instant, interactive_pressure: f64) -> CacheMaintenanceLease {
        let interactive_pressure = clamp_unit(interactive_pressure);
        self.refill(now, interactive_pressure);
        if interactive_pressure > 0.0 {
            self.rescore_tokens = self
                .rescore_tokens
                .max(self.policy.interactive_min_rescore_rows as f64)
                .min(self.policy.rescore_burst_rows as f64);
            self.fetch_request_tokens = self.fetch_request_tokens.max(1.0);
            self.fetch_byte_tokens = self
                .fetch_byte_tokens
                .max(self.policy.interactive_min_fetch_bytes as f64)
                .min(self.policy.fetch_byte_burst as f64);
        }

        let max_rescore = if interactive_pressure > 0.0 {
            self.policy
                .max_rescore_rows_per_lease
                .max(self.policy.interactive_min_rescore_rows)
        } else {
            self.policy.max_rescore_rows_per_lease
        };
        let rescore_total = self.take_whole_rescore_tokens(max_rescore);
        let stale_rescore_limit = if interactive_pressure > 0.0 {
            0
        } else {
            ((rescore_total as f64) * clamp_unit(self.policy.stale_rescore_fraction)).floor()
                as usize
        };
        let rescore_limit = rescore_total.saturating_sub(stale_rescore_limit);

        let in_backoff = self
            .backoff_until
            .is_some_and(|backoff_until| now < backoff_until);
        let fetch = if in_backoff {
            CacheFetchLease::none(interactive_pressure)
        } else {
            self.take_fetch_lease(interactive_pressure)
        };

        CacheMaintenanceLease {
            stale_rescore_limit,
            rescore_limit,
            fetch,
            network_rate_multiplier: self.network_rate_multiplier,
            in_backoff,
        }
    }

    pub fn record_feedback(
        &mut self,
        now: Instant,
        lease: &CacheMaintenanceLease,
        feedback: CacheMaintenanceFeedback,
    ) {
        self.refund_unused(lease, feedback);
        let fetch_failed = feedback.fetch_failed > 0 || feedback.had_fetch_error;
        if fetch_failed {
            self.consecutive_fetch_failures = self.consecutive_fetch_failures.saturating_add(1);
            self.network_rate_multiplier =
                (self.network_rate_multiplier * 0.5).max(self.policy.min_network_rate_multiplier);
            if feedback.fetch_cached == 0 {
                let exponent = self.consecutive_fetch_failures.saturating_sub(1).min(6);
                let backoff_seconds = 5_u64.saturating_mul(1_u64 << exponent).min(300);
                self.backoff_until = Some(now + Duration::from_secs(backoff_seconds));
            }
            return;
        }

        if feedback.fetch_attempted > 0 {
            self.consecutive_fetch_failures = 0;
            self.backoff_until = None;
            self.network_rate_multiplier = (self.network_rate_multiplier + 0.1).min(1.0);
        }
    }

    pub fn network_rate_multiplier(&self) -> f64 {
        self.network_rate_multiplier
    }

    pub fn is_in_backoff(&self, now: Instant) -> bool {
        self.backoff_until
            .is_some_and(|backoff_until| now < backoff_until)
    }

    fn refill(&mut self, now: Instant, interactive_pressure: f64) {
        let elapsed = now
            .saturating_duration_since(self.last_refill)
            .as_secs_f64();
        self.last_refill = now;
        if elapsed <= 0.0 {
            return;
        }
        self.rescore_tokens = (self.rescore_tokens + elapsed * self.policy.rescore_rows_per_second)
            .min(self.policy.rescore_burst_rows as f64);

        let request_rate = lerp(
            self.policy.background_fetch_requests_per_second,
            self.policy.interactive_fetch_requests_per_second,
            interactive_pressure,
        ) * self.network_rate_multiplier;
        self.fetch_request_tokens = (self.fetch_request_tokens + elapsed * request_rate)
            .min(self.policy.fetch_request_burst as f64);

        let byte_rate = lerp(
            self.policy.background_fetch_bytes_per_second,
            self.policy.interactive_fetch_bytes_per_second,
            interactive_pressure,
        ) * self.network_rate_multiplier;
        self.fetch_byte_tokens =
            (self.fetch_byte_tokens + elapsed * byte_rate).min(self.policy.fetch_byte_burst as f64);
    }

    fn take_whole_rescore_tokens(&mut self, max_rows: usize) -> usize {
        let rows = (self.rescore_tokens.floor() as usize).min(max_rows);
        self.rescore_tokens -= rows as f64;
        rows
    }

    fn take_fetch_lease(&mut self, interactive_pressure: f64) -> CacheFetchLease {
        let max_requests = lerp(
            self.policy.max_background_fetch_requests_per_lease as f64,
            self.policy.max_interactive_fetch_requests_per_lease as f64,
            interactive_pressure,
        )
        .round() as usize;
        let max_bytes = lerp(
            self.policy.max_background_fetch_bytes_per_lease as f64,
            self.policy.max_interactive_fetch_bytes_per_lease as f64,
            interactive_pressure,
        )
        .round() as u64;
        let request_limit = (self.fetch_request_tokens.floor() as usize).min(max_requests);
        let byte_limit = (self.fetch_byte_tokens.floor() as u64).min(max_bytes);
        if request_limit == 0 || byte_limit < self.policy.min_fetch_bytes_per_lease {
            return CacheFetchLease::none(interactive_pressure);
        }
        self.fetch_request_tokens -= request_limit as f64;
        self.fetch_byte_tokens -= byte_limit as f64;
        CacheFetchLease::new(request_limit, byte_limit, interactive_pressure)
    }

    fn refund_unused(&mut self, lease: &CacheMaintenanceLease, feedback: CacheMaintenanceFeedback) {
        let reserved_rescore = lease
            .stale_rescore_limit
            .saturating_add(lease.rescore_limit);
        let used_rescore = feedback
            .stale_rescore_queued
            .saturating_add(feedback.rescore_scanned)
            .min(reserved_rescore);
        self.rescore_tokens = (self.rescore_tokens
            + reserved_rescore.saturating_sub(used_rescore) as f64)
            .min(self.policy.rescore_burst_rows as f64);

        let used_requests = feedback.fetch_attempted.min(lease.fetch.request_limit);
        self.fetch_request_tokens = (self.fetch_request_tokens
            + lease.fetch.request_limit.saturating_sub(used_requests) as f64)
            .min(self.policy.fetch_request_burst as f64);

        let used_bytes = feedback.fetch_attempted_bytes.min(lease.fetch.byte_limit);
        self.fetch_byte_tokens = (self.fetch_byte_tokens
            + lease.fetch.byte_limit.saturating_sub(used_bytes) as f64)
            .min(self.policy.fetch_byte_burst as f64);
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
    pub rescore_priority: f64,
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

fn lerp(start: f64, end: f64, amount: f64) -> f64 {
    start + (end - start) * clamp_unit(amount)
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

    #[test]
    fn local_signal_rescore_priority_beats_background_work() {
        let priority = cache_signal_rescore_priority(&CacheSignalUpdate {
            account_id: "primary".to_string(),
            message_id: "message-1".to_string(),
            reason: "search-visible".to_string(),
            search: Some(CacheSearchSignals {
                total_messages: 1_000,
                result_count: 10,
                result_rank: 0,
            }),
            thread_activity: None,
            sender_affinity: None,
            local_behavior: None,
            direct_user_boost: Some(0.8),
            pinned: None,
        });

        assert!(priority > LOCAL_SIGNAL_RESCORE_BASE);
    }

    #[test]
    fn cache_resource_governor_caps_background_fetch_lease() {
        let now = Instant::now();
        let mut governor = CacheResourceGovernor::new(now, CacheResourcePolicy::default());

        let lease = governor.grant(now, 0.0);

        assert!(lease.rescore_limit > 0);
        assert!(lease.fetch.request_limit <= 2);
        assert!(lease.fetch.byte_limit <= 4 * 1024 * 1024);
        assert!(!lease.in_backoff);
    }

    #[test]
    fn cache_resource_governor_grants_interactive_burst() {
        let now = Instant::now();
        let mut governor = CacheResourceGovernor::new(now, CacheResourcePolicy::default());

        let lease = governor.grant(now, 1.0);

        assert!(lease.stale_rescore_limit == 0);
        assert!(lease.rescore_limit >= 200);
        assert!(lease.fetch.request_limit >= 1);
        assert!(lease.fetch.byte_limit >= 4 * 1024 * 1024);
    }

    #[test]
    fn cache_resource_governor_backs_off_after_failed_fetches() {
        let now = Instant::now();
        let mut governor = CacheResourceGovernor::new(now, CacheResourcePolicy::default());
        let lease = governor.grant(now, 1.0);

        governor.record_feedback(
            now,
            &lease,
            CacheMaintenanceFeedback {
                fetch_attempted: 1,
                fetch_attempted_bytes: 32 * 1024,
                fetch_failed: 1,
                ..Default::default()
            },
        );
        let backoff_lease = governor.grant(now + Duration::from_secs(1), 1.0);

        assert!(backoff_lease.in_backoff);
        assert_eq!(backoff_lease.fetch.request_limit, 0);
        assert!(governor.network_rate_multiplier() < 1.0);
    }

    #[test]
    fn cache_resource_governor_does_not_network_backoff_for_local_errors() {
        let now = Instant::now();
        let mut governor = CacheResourceGovernor::new(now, CacheResourcePolicy::default());
        let lease = governor.grant(now, 1.0);

        governor.record_feedback(
            now,
            &lease,
            CacheMaintenanceFeedback {
                had_error: true,
                ..Default::default()
            },
        );
        let next_lease = governor.grant(now + Duration::from_secs(1), 1.0);

        assert!(!next_lease.in_backoff);
        assert_eq!(governor.network_rate_multiplier(), 1.0);
    }

    #[test]
    fn cache_resource_governor_refunds_unused_lease() {
        let now = Instant::now();
        let mut governor = CacheResourceGovernor::new(now, CacheResourcePolicy::default());
        let lease = governor.grant(now, 0.0);

        governor.record_feedback(now, &lease, CacheMaintenanceFeedback::default());
        let next_lease = governor.grant(now, 0.0);

        assert_eq!(next_lease.fetch.request_limit, lease.fetch.request_limit);
        assert_eq!(next_lease.fetch.byte_limit, lease.fetch.byte_limit);
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
