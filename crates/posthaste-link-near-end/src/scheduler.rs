//! The engine's ambient timing + randomness, injected by the host.
//!
//! The engine needs to *wait* (deadlines, backoff sleeps) and to *jitter*
//! (decorrelate reconnect storms), but a wasm-pure crate cannot depend on a
//! timer (`tokio::time`) or an entropy source (`getrandom`) — those are host
//! IO, exactly like [`crate::transport::Transport`]. So they are a trait the
//! host supplies. The browser binding (in `posthaste-client-node-wasm`) backs `sleep`
//! with `setTimeout` and seeds `jitter` from a construction-time seed; tests
//! back both deterministically. Keeping these off the engine's own dependency
//! list is what lets `posthaste-link-near-end` stay on the wasm frontier.

use std::time::Duration;

use futures_util::future::LocalBoxFuture;

/// Host-supplied timing + jitter. Object-safe: held as `Rc<dyn Scheduler>`.
pub trait Scheduler {
    /// Resolve after `duration` has elapsed.
    fn sleep(&self, duration: Duration) -> LocalBoxFuture<'static, ()>;

    /// A uniform random value in `[0.0, 1.0)` for backoff jitter. Need not be
    /// cryptographic — only decorrelation matters.
    fn jitter(&self) -> f64;
}
