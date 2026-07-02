use super::*;

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
