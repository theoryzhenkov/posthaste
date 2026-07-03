//! The typed outcome of a provider call attempt — carries the shared
//! [`Terminality`] verdict (so the retry loop and the breaker agree on what is
//! retryable) plus the transport-level reason and any parsed `Retry-After`.

use std::time::Duration;

use posthaste_call_policy::Terminality;

/// A failed provider call, classified once at the transport edge (D82).
///
/// The [`terminality`](Self::terminality) is the single axis the retry loop and
/// the circuit breaker consume: only [`Terminality::Transient`] outcomes retry,
/// and the breaker counts a call as failed once its terminal outcome is an
/// `Err` of this type. [`retry_after`](Self::retry_after) is the server's parsed
/// backpressure (429/503), fed verbatim into the policy's retry arithmetic.
#[derive(Clone, Debug, thiserror::Error)]
#[error("provider call failed [{reason:?} / {terminality:?}]: {detail}")]
pub struct ProviderCallError {
    /// Whether a retry may succeed. Drives both the retry loop and breaker.
    pub terminality: Terminality,
    /// The transport-level reason, for status surfacing and operator triage.
    pub reason: CallErrorReason,
    /// Parsed `Retry-After` (429/503), honored verbatim by the policy.
    pub retry_after: Option<Duration>,
    /// Human-readable detail (status line, transport message, or breaker reason).
    pub detail: String,
}

/// Why a provider call failed — a transport-level taxonomy the executor can see
/// without parsing the protocol body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallErrorReason {
    /// The per-account circuit breaker is open; the call was short-circuited
    /// (D83). Distinct so callers/status can surface "provider circuit open".
    CircuitOpen,
    /// The per-class *total* deadline elapsed (metadata/send).
    Timeout,
    /// The blob *stall* deadline elapsed: no bytes arrived within the window
    /// (F2's real fix — a slow-but-progressing body is *not* this).
    Stall,
    /// A non-2xx HTTP status (the `u16` is the status).
    Http(u16),
    /// A rate-limit / overload status (429/503); carries the status and, on the
    /// error, the parsed `Retry-After`.
    RateLimited(u16),
    /// A transport fault before/around a usable response (DNS/connect/TLS/read).
    Transport,
}

impl ProviderCallError {
    /// The fast-fail error returned when the breaker is open for `account` — a
    /// [`Terminality::Transient`] carrying a distinct reason so the caller (and
    /// account status) can say "provider circuit open" rather than a generic
    /// network error (D83).
    pub fn circuit_open(account: &str) -> Self {
        Self {
            terminality: Terminality::Transient,
            reason: CallErrorReason::CircuitOpen,
            retry_after: None,
            detail: format!("provider circuit open for account {account}"),
        }
    }

    /// Whether this outcome is retry-eligible.
    pub fn is_transient(&self) -> bool {
        self.terminality.is_transient()
    }

    pub(crate) fn timeout(detail: impl Into<String>) -> Self {
        Self {
            terminality: Terminality::Transient,
            reason: CallErrorReason::Timeout,
            retry_after: None,
            detail: detail.into(),
        }
    }

    pub(crate) fn stall(detail: impl Into<String>) -> Self {
        Self {
            terminality: Terminality::Transient,
            reason: CallErrorReason::Stall,
            retry_after: None,
            detail: detail.into(),
        }
    }

    pub(crate) fn transport(detail: impl Into<String>) -> Self {
        Self {
            terminality: Terminality::Transient,
            reason: CallErrorReason::Transport,
            retry_after: None,
            detail: detail.into(),
        }
    }
}
