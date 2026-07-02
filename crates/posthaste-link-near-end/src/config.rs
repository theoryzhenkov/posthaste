//! Engine policy constants — the one place the near-end's resilience is tuned.
//!
//! These defaults replace three broken forks (lifecycle-debt rows 1, 2, 8: no
//! deadline on the runtime→authority link, no reconnect behind the down-channel,
//! the web client's flat-1s retry). **Every value here is flagged for review**
//! — they are sane starting points, not measured optima.

use std::time::Duration;

/// Jittered, capped exponential backoff (AWS "full jitter": the actual sleep is
/// `random(0, min(cap, base * factor^attempt))`). Full jitter — not the flat 1s
/// the TS client used — is what decorrelates a thundering-herd reconnect after a
/// shared authority-server blip.
#[derive(Clone, Debug)]
pub struct BackoffPolicy {
    /// First-attempt ceiling before jitter. **Review.**
    pub base: Duration,
    /// Growth factor per attempt. **Review.**
    pub factor: f64,
    /// Absolute ceiling on the pre-jitter delay. **Review.**
    pub cap: Duration,
}

impl Default for BackoffPolicy {
    fn default() -> Self {
        Self {
            base: Duration::from_millis(500),
            factor: 2.0,
            cap: Duration::from_secs(30),
        }
    }
}

impl BackoffPolicy {
    /// The pre-jitter ceiling for a 0-based `attempt`: `min(cap, base*factor^n)`.
    pub fn ceiling(&self, attempt: u32) -> Duration {
        let base_ms = self.base.as_secs_f64() * 1000.0;
        let grown_ms = base_ms * self.factor.powi(attempt as i32);
        let cap_ms = self.cap.as_secs_f64() * 1000.0;
        Duration::from_millis(grown_ms.min(cap_ms).max(0.0) as u64)
    }

    /// The jittered sleep for a 0-based `attempt`: `random_unit * ceiling`.
    /// `random_unit` comes from [`crate::scheduler::Scheduler::jitter`].
    pub fn sleep_for(&self, attempt: u32, random_unit: f64) -> Duration {
        let ceiling_ms = self.ceiling(attempt).as_secs_f64() * 1000.0;
        let clamped = random_unit.clamp(0.0, 1.0);
        Duration::from_millis((ceiling_ms * clamped) as u64)
    }
}

/// The near-end engine's policy tunables. Constructed once and held for the
/// engine's life. Wire-shape settings (base path, session options) live on the
/// seam's [`crate::wire::Wire`] profile, not here — this is pure policy.
#[derive(Clone, Debug)]
pub struct NearEndConfig {
    /// Wall-clock ceiling on a single request attempt (forward POST, prepare
    /// POST, settlement GET). A hung far node no longer wedges the pipeline
    /// forever (row 1). **Review** (default 30s).
    pub request_deadline: Duration,
    /// How many times a *transient* forward failure is retried (with backoff)
    /// before giving up and surfacing the error. Permanent (4xx) failures are
    /// never retried. **Review** (default 4).
    pub forward_max_attempts: u32,
    /// Backoff shared by forward-retry and stream-reconnect. **Review.**
    pub backoff: BackoffPolicy,
    /// Resume cursor to seed on construction — the last frame seq the host
    /// persisted from a prior run. The engine owns the cursor thereafter
    /// ([`crate::engine::NearEnd::cursor`]); callers no longer thread `afterSeq`.
    pub initial_cursor: Option<u64>,
}

impl Default for NearEndConfig {
    fn default() -> Self {
        Self {
            request_deadline: Duration::from_secs(30),
            forward_max_attempts: 4,
            backoff: BackoffPolicy::default(),
            initial_cursor: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ceiling_grows_then_caps() {
        let b = BackoffPolicy {
            base: Duration::from_millis(500),
            factor: 2.0,
            cap: Duration::from_secs(30),
        };
        assert_eq!(b.ceiling(0), Duration::from_millis(500));
        assert_eq!(b.ceiling(1), Duration::from_millis(1000));
        assert_eq!(b.ceiling(2), Duration::from_millis(2000));
        // Caps well before overflow.
        assert_eq!(b.ceiling(20), Duration::from_secs(30));
    }

    #[test]
    fn full_jitter_scales_the_ceiling() {
        let b = BackoffPolicy::default();
        // random_unit 0 → no sleep; 1 → the full ceiling; 0.5 → half.
        assert_eq!(b.sleep_for(1, 0.0), Duration::from_millis(0));
        assert_eq!(b.sleep_for(1, 1.0), b.ceiling(1));
        assert_eq!(b.sleep_for(1, 0.5), Duration::from_millis(500));
        // Out-of-range jitter is clamped, never panics.
        assert_eq!(b.sleep_for(0, 2.0), b.ceiling(0));
        assert_eq!(b.sleep_for(0, -1.0), Duration::from_millis(0));
    }
}
