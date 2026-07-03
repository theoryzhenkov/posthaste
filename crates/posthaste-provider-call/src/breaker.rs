//! The per-account circuit breaker (D83, R86).
//!
//! Keyed by account, **never global**: one account's expired cert or rate-limit
//! must not fast-fail every healthy account. After
//! [`BREAKER_FAILURE_THRESHOLD`] consecutive failed calls the breaker opens for
//! [`BREAKER_COOLDOWN`]; while open, calls are short-circuited (fast-fail). After
//! the cooldown a *single* half-open probe is admitted — it closes the breaker on
//! success or re-opens it on failure. State lives in the executor instance and is
//! reset on any success.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use tokio::time::Instant;

/// Consecutive-failure count that opens the breaker (O4: ratified 5).
pub const BREAKER_FAILURE_THRESHOLD: u32 = 5;
/// How long the breaker stays open before admitting a half-open probe (O4:
/// ratified 30–60 s; the mid-band concrete value).
pub const BREAKER_COOLDOWN: Duration = Duration::from_secs(45);

/// Tunable circuit-breaker parameters (O4). `enabled` is the kill switch: with
/// the breaker off, every call is admitted and no state is tracked, so the
/// envelope's retry/deadline behavior can be exercised in isolation.
#[derive(Clone, Copy, Debug)]
pub struct BreakerConfig {
    /// The breaker flag (D83/O4): `false` disables short-circuiting entirely.
    pub enabled: bool,
    /// Consecutive failures that open the breaker.
    pub failure_threshold: u32,
    /// Open-state cooldown before a half-open probe is admitted.
    pub cooldown: Duration,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            failure_threshold: BREAKER_FAILURE_THRESHOLD,
            cooldown: BREAKER_COOLDOWN,
        }
    }
}

/// A read-only view of an account's breaker phase (for status surfacing/tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakerPhaseView {
    /// Calls flow normally; the payload is the current consecutive-failure count.
    Closed(u32),
    /// The breaker is open and short-circuiting calls.
    Open,
    /// A single probe is in flight (or admissible after cooldown).
    HalfOpen,
}

#[derive(Clone, Copy, Debug)]
enum Phase {
    Closed(u32),
    Open(Instant),
    HalfOpen,
}

/// The per-account breaker table. Cheap to lock (a brief `HashMap` touch at the
/// start and end of each call) and self-reaping — an account's entry resets to
/// `Closed(0)` on any success.
pub(crate) struct BreakerRegistry {
    config: BreakerConfig,
    phases: Mutex<HashMap<String, Phase>>,
}

impl BreakerRegistry {
    pub(crate) fn new(config: BreakerConfig) -> Self {
        Self {
            config,
            phases: Mutex::new(HashMap::new()),
        }
    }

    /// Decide whether a call may proceed, mutating the phase where admitting a
    /// probe requires the Open→HalfOpen transition. Returns `false` when the
    /// breaker is open (still cooling) or a probe is already in flight.
    pub(crate) fn admit(&self, account: &str) -> bool {
        if !self.config.enabled {
            return true;
        }
        let mut phases = self.phases.lock().expect("breaker mutex poisoned");
        match phases.get(account).copied() {
            None | Some(Phase::Closed(_)) => true,
            Some(Phase::Open(opened_at)) => {
                if opened_at.elapsed() >= self.config.cooldown {
                    // Cooldown elapsed: admit exactly one probe.
                    phases.insert(account.to_string(), Phase::HalfOpen);
                    true
                } else {
                    false
                }
            }
            // A probe is already in flight — reject the rest (single probe).
            Some(Phase::HalfOpen) => false,
        }
    }

    /// Record a call's terminal outcome. Success closes/resets the breaker; a
    /// failure trips it (a half-open probe failure re-opens, a closed-state run
    /// increments and opens on threshold).
    pub(crate) fn record(&self, account: &str, success: bool) {
        if !self.config.enabled {
            return;
        }
        let mut phases = self.phases.lock().expect("breaker mutex poisoned");
        if success {
            phases.insert(account.to_string(), Phase::Closed(0));
            return;
        }
        let next = match phases.get(account).copied() {
            Some(Phase::HalfOpen) | Some(Phase::Open(_)) => Phase::Open(Instant::now()),
            None | Some(Phase::Closed(_)) => {
                let failures = match phases.get(account) {
                    Some(Phase::Closed(n)) => *n + 1,
                    _ => 1,
                };
                if failures >= self.config.failure_threshold {
                    Phase::Open(Instant::now())
                } else {
                    Phase::Closed(failures)
                }
            }
        };
        phases.insert(account.to_string(), next);
    }

    /// The current phase view for `account` (defaults to `Closed(0)` if unseen).
    pub(crate) fn phase(&self, account: &str) -> BreakerPhaseView {
        let phases = self.phases.lock().expect("breaker mutex poisoned");
        match phases.get(account).copied() {
            None => BreakerPhaseView::Closed(0),
            Some(Phase::Closed(n)) => BreakerPhaseView::Closed(n),
            Some(Phase::Open(_)) => BreakerPhaseView::Open,
            Some(Phase::HalfOpen) => BreakerPhaseView::HalfOpen,
        }
    }
}
