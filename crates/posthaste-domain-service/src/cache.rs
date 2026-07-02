use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use posthaste_domain_model::*;

const MIB: f64 = 1024.0 * 1024.0;
const MIN_BILLABLE_BYTES: f64 = 4.0 * 1024.0;
const PINNED_BONUS: f64 = 4.0;
const LOCAL_SIGNAL_RESCORE_BASE: f64 = 100.0;

/// Tunable manual weights for optional local cache utility scoring.
///
/// @spec docs/L1-sync#local-cache-planning
mod resource_governor;
mod scoring;

pub use resource_governor::CacheResourceGovernor;
pub use scoring::{
    cache_signal_rescore_priority, decide_cache_admission, score_cache_candidate,
    score_cache_candidate_with_weights, CacheAdmission,
};

pub(crate) use scoring::{lerp};

#[cfg(test)]
mod tests;
