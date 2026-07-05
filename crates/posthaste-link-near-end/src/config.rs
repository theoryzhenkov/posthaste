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

/// The server's SSE keep-alive cadence. Axum's `KeepAlive::default()` writes a
/// `:\n\n` comment every **15s** on an otherwise-idle stream
/// (`crates/posthaste-http-api-adapter/src/api/runtime_stream/links.rs:106`;
/// `axum::response::sse::KeepAlive` default `max_interval`). The client's
/// `fetch-event-source` shim dispatches the comment's trailing blank line as an
/// **empty** message, so every keep-alive reaches the engine as an empty
/// [`crate::transport::StreamEvent::Message`] — i.e. a live-but-idle stream
/// still ticks the frame loop every 15s (proof of life), not just a busy one.
pub(crate) const SERVER_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);

/// W1 — the read-liveness deadline. A live link produces *bytes* (a frame OR a
/// keep-alive) at least once per [`SERVER_KEEP_ALIVE_INTERVAL`]; total silence
/// for **3×** that means the socket is silently half-open (laptop sleep/wake,
/// NAT/Wi-Fi rebind, a proxy that dropped the connection with no RST) and
/// `stream.next()` will block forever with no error. The engine arms this as a
/// read deadline and, on expiry, re-prepares the link (the same M40/M44 path a
/// stale-link 404 uses). 3× keep-alive → a single dropped keep-alive can never
/// false-trip an idle-but-alive stream.
pub(crate) const STREAM_LIVENESS_DEADLINE: Duration =
    Duration::from_secs(3 * SERVER_KEEP_ALIVE_INTERVAL.as_secs());

// The deadline must span at least a couple of keep-alives, or a genuinely
// idle-but-alive stream (keep-alives every 15s) would false-trip the watchdog.
const _: () = assert!(
    STREAM_LIVENESS_DEADLINE.as_secs() >= 2 * SERVER_KEEP_ALIVE_INTERVAL.as_secs(),
    "the liveness deadline must span >= two keep-alives or an idle-but-alive stream false-trips",
);

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
    /// W1 — wall-clock ceiling on **total silence** from the frame stream (no
    /// frame *and* no keep-alive) before the engine treats the link as silently
    /// dead and re-prepares it. Seeded from [`STREAM_LIVENESS_DEADLINE`] (3× the
    /// server keep-alive). The one gap the observed-`Error` recovery (M40/M44)
    /// can never see: a half-open socket yields no error, so the read loop would
    /// otherwise block forever. **Review** (default 45s).
    pub stream_liveness_deadline: Duration,
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
            stream_liveness_deadline: STREAM_LIVENESS_DEADLINE,
        }
    }
}
