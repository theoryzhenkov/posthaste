use std::time::Duration;

use serde::{Deserialize, Serialize};

const DEFAULT_SIZE_ALPHA: f64 = 0.7;
const BYTES_PER_MIB: f64 = 1024.0 * 1024.0;

/// Tunable manual weights for optional local cache utility scoring.
///
/// @spec docs/L1-sync#local-cache-planning
mod budget;
mod entities;
mod primitives;
mod resource_types;

pub use budget::{CacheBudget, clamp_unit};
pub use entities::{
    CacheCandidate, CacheCandidateSignals, CacheFetchCandidate, CacheMessageSignals, CacheObject,
    CachePriorityUpdate, CacheRescoreCandidate, CacheScore, CacheSearchSignals, CacheSignalUpdate,
};
pub use primitives::{
    CacheFetchUnit, CacheLayer, CacheObjectState, CachePolicy, CacheScoringWeights,
};
pub use resource_types::{
    CacheFetchLease, CacheMaintenanceFeedback, CacheMaintenanceLease, CacheResourcePolicy,
};
