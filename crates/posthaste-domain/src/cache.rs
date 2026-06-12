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
mod entities;
mod primitives;
mod resource_governor;
mod resource_types;
mod scoring;

pub use entities::{
    CacheCandidate, CacheCandidateSignals, CacheFetchCandidate, CacheMessageSignals, CacheObject,
    CachePriorityUpdate, CacheRescoreCandidate, CacheScore, CacheSearchSignals, CacheSignalUpdate,
};
pub use primitives::{
    CacheFetchUnit, CacheLayer, CacheObjectState, CachePolicy, CacheScoringWeights,
};
pub use resource_governor::CacheResourceGovernor;
pub use resource_types::{
    CacheFetchLease, CacheMaintenanceFeedback, CacheMaintenanceLease, CacheResourcePolicy,
};
pub use scoring::{
    cache_signal_rescore_priority, decide_cache_admission, score_cache_candidate,
    score_cache_candidate_with_weights, CacheAdmission, CacheBudget,
};

pub(crate) use scoring::{clamp_unit, lerp};

#[cfg(test)]
mod tests;
