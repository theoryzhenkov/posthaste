//! Server-side request/stream deadline constants (RFC-L2-lifecycle-and-errors
//! D64 / migration M24).
//!
//! Before this migration the only deadline anywhere in the adapter was the TLS
//! handshake (`tls.rs`) — every HTTP handler await was unbounded (audit N10).
//! Two deadline shapes close that gap, named once here so the value is a
//! single tuning surface:
//!
//! * [`REQUEST_TIMEOUT`] — the blanket [`tower_http::timeout::TimeoutLayer`]
//!   `crate::router::build_api_router` applies to every regular (non-SSE)
//!   `/v1` route.
//! * [`STREAM_SETUP_TIMEOUT`] — each SSE handler's *setup* await (the runtime
//!   call that produces the stream/subscription: open/subscribe/catch-up) is
//!   wrapped in this deadline via [`with_stream_setup_deadline`]. The
//!   streaming phase itself is deliberately **not** deadline-bounded here — a
//!   stream is supposed to live long; keepalive/idle-reap of a stalled stream
//!   is D68's reaper, not this migration's unit.
//!
//! **Why this does not reuse `posthaste-call-policy`'s `CallClass` table.**
//! That crate's deadline vocabulary (D81) is the *outbound* policy the near-end
//! engine and the native provider executor share for calls this process makes
//! *to* a provider/link peer (client role). These constants govern the
//! opposite direction — calls this process's own HTTP boundary makes *into*
//! its own runtime while serving an inbound request (server role). Same
//! discipline (named constants, one place, tunable), different actor; adding
//! `posthaste-call-policy` as a dependency here just to borrow its `Duration`
//! values would blur that boundary for no shared behavior, so the constants
//! are local to this crate instead (reported per the migration brief).
//!
//! Both values below are **review-flagged defaults**, not measurements:
//! `REQUEST_TIMEOUT` matches the RFC's suggested "~30s, aligns with Metadata"
//! default; `STREAM_SETUP_TIMEOUT` is tighter because SSE setup is in-process
//! bookkeeping (session/subscription lookup), not a network round trip, so it
//! should resolve quickly or not at all.

use std::future::Future;
use std::time::Duration;

use axum::http::StatusCode;

use crate::api::{ApiError, ApiErrorCode};

/// Blanket timeout for regular (non-streaming) `/v1` request/response routes
/// (D64). **Review**: 30s, aligned with `posthaste-call-policy`'s
/// `CallClass::Metadata` total deadline. Note: `trigger_sync`'s manual sync
/// call is a regular route under this blanket and can legitimately run long
/// for a large backfill — flagged here, not specially carved out, by this
/// migration; revisit if that proves too tight.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Manual sync (`POST .../commands/sync`) awaits the full provider sync cycle
/// end-to-end, so it legitimately outlives [`REQUEST_TIMEOUT`] on large
/// mailboxes. Bounded, but generously — the deeper fix (enqueue-and-return
/// semantics) is provider-reliability M36 territory.
pub const SYNC_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Deadline for each SSE handler's *setup* await — the runtime call that
/// produces the stream/subscription (open/subscribe/catch-up) — not the
/// streaming phase itself. **Review**: 10s.
pub const STREAM_SETUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Wrap an SSE handler's setup await (the runtime call that produces the
/// stream/subscription) with [`STREAM_SETUP_TIMEOUT`]. `what` names the call
/// for the error message (e.g. `"runtime frame subscription"`).
///
/// Used by `api::runtime_stream::links::stream_runtime_link` and
/// `api::sync_events::stream_events` — the two SSE handlers excluded from the
/// blanket [`REQUEST_TIMEOUT`] layer.
pub(crate) async fn with_stream_setup_deadline<F, T>(what: &str, future: F) -> Result<T, ApiError>
where
    F: Future<Output = Result<T, ApiError>>,
{
    match tokio::time::timeout(STREAM_SETUP_TIMEOUT, future).await {
        Ok(result) => result,
        Err(_) => Err(request_timeout_error(what)),
    }
}

/// The shared timeout → `ApiError` mapping, used by both
/// [`with_stream_setup_deadline`] and the blanket `TimeoutLayer`'s error
/// handler in `crate::router`: a timeout is Transient (M29 vocabulary) —
/// reuse the existing `GatewayUnavailable`/503 pairing already used for
/// `RuntimeErrorCode::ProviderUnavailable` rather than inventing a new code
/// (boundary sanitization of the message text is M30's unit, not this one's).
pub(crate) fn request_timeout_error(what: &str) -> ApiError {
    ApiError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        ApiErrorCode::GatewayUnavailable,
        format!("{what} exceeded its deadline"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn a_hanging_setup_await_is_bounded_not_hung() {
        // The wedge-prone case D64/M24 exists to fix: a runtime call inside SSE
        // setup that never resolves. With the clock paused the deadline fires
        // virtually — this must complete, not hang the test.
        let result: Result<(), ApiError> =
            with_stream_setup_deadline("test subscription", std::future::pending()).await;
        let err = result.expect_err("a wedged setup await must time out, not hang");
        assert_eq!(err.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(err.body.code, ApiErrorCode::GatewayUnavailable);
        assert!(err.body.message.contains("test subscription"));
    }

    #[tokio::test]
    async fn a_fast_setup_await_passes_through_unaffected() {
        let result =
            with_stream_setup_deadline("test subscription", async { Ok::<_, ApiError>(42) }).await;
        match result {
            Ok(value) => assert_eq!(value, 42),
            Err(_) => panic!("a fast setup await must not be perturbed"),
        }
    }
}
