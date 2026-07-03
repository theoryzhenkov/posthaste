//! Engine policy config — the near-end's tunables, now sourced from the shared
//! [`posthaste_call_policy`] policy core.
//!
//! These defaults replace three broken forks (lifecycle-debt rows 1, 2, 8: no
//! deadline on the runtime→authority link, no reconnect behind the down-channel,
//! the web client's flat-1s retry). Post-M30 the *arithmetic* they configure —
//! the jittered capped exponential backoff and the per-class deadline table —
//! lives in `posthaste-call-policy` (D80); this struct just selects the values
//! the near-end runs on. **Every value here is flagged for review** — sane
//! starting points, not measured optima.

use std::time::Duration;

use posthaste_call_policy::{BackoffSchedule, CallClass};

/// Re-exported: the jittered, capped exponential backoff schedule now lives in
/// the shared policy core so the link engine and the native provider executor
/// share one schedule (D80). The near-end holds one on [`NearEndConfig`].
pub use posthaste_call_policy::BackoffSchedule as BackoffPolicy;

/// The near-end engine's policy tunables. Constructed once and held for the
/// engine's life. Wire-shape settings (base path, link options) live on the
/// seam's [`crate::wire::Wire`] profile, not here — this is pure policy.
#[derive(Clone, Debug)]
pub struct NearEndConfig {
    /// Wall-clock ceiling on a single request attempt (forward POST, prepare
    /// POST, settlement GET). A hung far node no longer wedges the pipeline
    /// forever (row 1). Seeded from the policy core's metadata-class total
    /// deadline (D81); **Review** (default 30s).
    pub request_deadline: Duration,
    /// How many times a *transient* forward failure is retried (with backoff)
    /// before giving up and surfacing the error. Permanent (4xx) failures are
    /// never retried. **Review** (default 4).
    pub forward_max_attempts: u32,
    /// Backoff shared by forward-retry and stream-reconnect — the shared
    /// [`BackoffSchedule`] from the policy core. **Review.**
    pub backoff: BackoffSchedule,
    /// Resume cursor to seed on construction — the last frame seq the host
    /// persisted from a prior run. The engine owns the cursor thereafter
    /// ([`crate::engine::NearEnd::cursor`]); callers no longer thread `afterSeq`.
    pub initial_cursor: Option<u64>,
    /// How many **consecutive** malformed frames the engine tolerates before it
    /// stops treating them as ignorable keep-alives and declares the wire
    /// permanently broken — a version skew / corrupt peer ([3]). At the threshold
    /// it surfaces [`crate::sink::ConnectionStatus::Degraded`] and stops the loop.
    /// **Review** (default 3).
    pub max_consecutive_malformed: u32,
}

impl Default for NearEndConfig {
    fn default() -> Self {
        Self {
            // The near-end's request is a metadata-class call; take its total
            // deadline from the policy core's one deadline table (D81).
            request_deadline: CallClass::Metadata
                .deadline_policy()
                .total
                .expect("metadata class has a total deadline"),
            forward_max_attempts: 4,
            backoff: BackoffSchedule::default(),
            initial_cursor: None,
            max_consecutive_malformed: 3,
        }
    }
}
