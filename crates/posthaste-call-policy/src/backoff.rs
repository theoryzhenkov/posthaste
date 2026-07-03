//! Jittered, capped exponential backoff + the `Retry-After`/429 arithmetic.
//!
//! This is the arithmetic half of the outbound-call retry policy (D80/D81/F1),
//! extracted verbatim from `posthaste-link-near-end`'s scheduler so the link
//! engine and the (M31) native provider executor share one schedule instead of
//! forking it. It is pure: no timer, no RNG, no clock. The caller supplies both
//! the `attempt` counter and the jitter `rand_unit` — see [`BackoffSchedule::delay_for`].

use std::time::Duration;

/// AWS-style **full-jitter** capped exponential backoff, plus a `max_attempts`
/// give-up bound.
///
/// The pre-jitter ceiling for a 0-based attempt is `min(cap, base * factor^attempt)`;
/// the actual delay is `rand_unit * ceiling` (full jitter). Full jitter — not a
/// flat retry — is what decorrelates a thundering-herd reconnect/retry after a
/// shared endpoint blip (D89/Sc1).
///
/// `max_attempts` bounds the retry loop: it is the give-up policy the
/// `Retry-After` arithmetic ([`BackoffSchedule::retry_delay`]) enforces.
#[derive(Clone, Debug, PartialEq)]
pub struct BackoffSchedule {
    /// First-attempt ceiling before jitter. **Review** (tunable).
    pub base: Duration,
    /// Growth factor per attempt. **Review.**
    pub factor: f64,
    /// Absolute ceiling on the pre-jitter delay. **Review.**
    pub cap: Duration,
    /// How many attempts before the retry loop gives up. `0`/`1` mean "no retry".
    /// **Review.**
    pub max_attempts: u32,
}

/// The verdict of the `Retry-After`-aware retry arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryDecision {
    /// Sleep this long, then re-attempt.
    Retry(Duration),
    /// The give-up bound (`max_attempts`) is reached — stop and surface the error.
    GiveUp,
}

impl Default for BackoffSchedule {
    fn default() -> Self {
        Self {
            base: DEFAULT_BASE,
            factor: DEFAULT_FACTOR,
            cap: DEFAULT_CAP,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
        }
    }
}

/// First-attempt ceiling before jitter. **Review.**
pub const DEFAULT_BASE: Duration = Duration::from_millis(500);
/// Growth factor per attempt. **Review.**
pub const DEFAULT_FACTOR: f64 = 2.0;
/// Absolute ceiling on the pre-jitter delay. **Review.**
pub const DEFAULT_CAP: Duration = Duration::from_secs(30);
/// Default give-up bound for a transient-forward retry loop. **Review.**
pub const DEFAULT_MAX_ATTEMPTS: u32 = 4;

impl BackoffSchedule {
    /// The pre-jitter ceiling for a 0-based `attempt`: `min(cap, base*factor^n)`.
    /// Saturates at `cap` well before any float overflow.
    pub fn ceiling(&self, attempt: u32) -> Duration {
        let base_ms = self.base.as_secs_f64() * 1000.0;
        let grown_ms = base_ms * self.factor.powi(attempt as i32);
        let cap_ms = self.cap.as_secs_f64() * 1000.0;
        Duration::from_millis(grown_ms.min(cap_ms).max(0.0) as u64)
    }

    /// The jittered delay for a 0-based `attempt`: `rand_unit * ceiling`.
    ///
    /// `rand_unit` is a uniform value the **caller** draws from `[0.0, 1.0)`
    /// (out-of-range values are clamped, never panic). Randomness is injected —
    /// not `thread_rng`/`Math.random` inside — because this crate is wasm-pure
    /// and every retry decision must be deterministically reproducible in a test.
    pub fn delay_for(&self, attempt: u32, rand_unit: f64) -> Duration {
        let ceiling_ms = self.ceiling(attempt).as_secs_f64() * 1000.0;
        let clamped = rand_unit.clamp(0.0, 1.0);
        Duration::from_millis((ceiling_ms * clamped) as u64)
    }

    /// The `Retry-After`-aware retry decision for a 0-based `attempt` (D81/F1).
    ///
    /// * If `attempt` has reached `max_attempts`, the give-up policy fires
    ///   ([`RetryDecision::GiveUp`]).
    /// * Otherwise the delay is `max(retry_after, jittered_backoff)`: a server
    ///   that sent `Retry-After` (429/503) is **never re-hammered early** — F1's
    ///   whole point — but a *shorter* `Retry-After` never undercuts the jittered
    ///   backoff either. `retry_after` (already parsed from seconds or an
    ///   http-date to a `Duration` by the caller) is honored verbatim and is
    ///   deliberately **not** clamped down to `cap`: clamping it would defeat the
    ///   server's explicit backpressure. The `cap` bounds only the computed
    ///   backoff arm.
    pub fn retry_delay(
        &self,
        attempt: u32,
        rand_unit: f64,
        retry_after: Option<Duration>,
    ) -> RetryDecision {
        if attempt >= self.max_attempts {
            return RetryDecision::GiveUp;
        }
        let backoff = self.delay_for(attempt, rand_unit);
        let delay = match retry_after {
            Some(after) => after.max(backoff),
            None => backoff,
        };
        RetryDecision::Retry(delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule() -> BackoffSchedule {
        BackoffSchedule {
            base: Duration::from_millis(500),
            factor: 2.0,
            cap: Duration::from_secs(30),
            max_attempts: 4,
        }
    }

    #[test]
    fn ceiling_grows_then_caps() {
        let b = schedule();
        assert_eq!(b.ceiling(0), Duration::from_millis(500));
        assert_eq!(b.ceiling(1), Duration::from_millis(1000));
        assert_eq!(b.ceiling(2), Duration::from_millis(2000));
        // Caps well before overflow.
        assert_eq!(b.ceiling(20), Duration::from_secs(30));
    }

    #[test]
    fn full_jitter_scales_the_ceiling() {
        let b = BackoffSchedule::default();
        // rand_unit 0 → no delay; 1 → the full ceiling; 0.5 → half.
        assert_eq!(b.delay_for(1, 0.0), Duration::from_millis(0));
        assert_eq!(b.delay_for(1, 1.0), b.ceiling(1));
        assert_eq!(b.delay_for(1, 0.5), Duration::from_millis(500));
        // Out-of-range jitter is clamped, never panics.
        assert_eq!(b.delay_for(0, 2.0), b.ceiling(0));
        assert_eq!(b.delay_for(0, -1.0), Duration::from_millis(0));
    }

    #[test]
    fn retry_gives_up_at_max_attempts() {
        let b = schedule();
        assert_eq!(b.retry_delay(4, 0.5, None), RetryDecision::GiveUp);
        assert_eq!(b.retry_delay(5, 0.5, None), RetryDecision::GiveUp);
        assert!(matches!(b.retry_delay(3, 0.5, None), RetryDecision::Retry(_)));
    }

    #[test]
    fn retry_after_floors_the_backoff() {
        let b = schedule();
        // A long Retry-After wins over the (capped, jittered) backoff and is
        // honored verbatim — even beyond `cap` (server backpressure, F1).
        let long = Duration::from_secs(120);
        assert_eq!(
            b.retry_delay(0, 1.0, Some(long)),
            RetryDecision::Retry(long)
        );
        // A short Retry-After never undercuts the jittered backoff.
        let backoff = b.delay_for(2, 1.0);
        assert_eq!(
            b.retry_delay(2, 1.0, Some(Duration::from_millis(1))),
            RetryDecision::Retry(backoff)
        );
    }

    #[test]
    fn retry_without_retry_after_is_plain_backoff() {
        let b = schedule();
        assert_eq!(
            b.retry_delay(1, 0.5, None),
            RetryDecision::Retry(b.delay_for(1, 0.5))
        );
    }
}
