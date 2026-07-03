//! The shared outbound-call **policy core** (RFC-L2-provider-reliability D80).
//!
//! One place for the wasm-pure *policy* — the arithmetic and types that decide
//! how an outbound provider or link call retries, backs off, deadlines, and
//! classifies — so the link engine (`posthaste-link-near-end`) and the (M31)
//! native provider executor (`posthaste-provider-call`) consume one shared fact
//! rather than forking it three times (tenet XIV; R80: share the *policy*, not
//! the whole link *engine*).
//!
//! Four pieces, each a pure function/table:
//!
//! * [`BackoffSchedule`] — AWS full-jitter capped exponential backoff +
//!   `Retry-After`/429 arithmetic ([`BackoffSchedule::retry_delay`]) + a
//!   `max_attempts` give-up policy (D81/F1).
//! * [`CallClass`] → [`DeadlinePolicy`] — the per-class deadline table that
//!   retires F2's 10 s monoculture: metadata gets a total, blob a stall, send a
//!   dispatch-coupled total, subscribe a connect-only deadline (D81).
//! * [`classify_status`] / [`resolve_terminality`] — the status-band + envelope
//!   **precedence rule** over the shared [`Terminality`] taxonomy (D82; O2: the
//!   taxonomy is owned by `posthaste-domain-model`, consumed here, never forked).
//!
//! **Why the crate is dependency-thin and takes explicit inputs.** No tokio, no
//! reqwest, no serde, no ambient clock, no RNG: the *execution* half (sleeping a
//! timer, drawing randomness, wrapping a call in `tokio::time::timeout`) belongs
//! to the host/executor; only the *arithmetic* lives here. Every entry point
//! takes an explicit `attempt` and `rand_unit`, so a retry decision is
//! deterministic and unit-testable, and the crate compiles to
//! `wasm32-unknown-unknown` on the D15 frontier.

mod backoff;
mod classify;
mod deadline;

pub use backoff::{
    BackoffSchedule, RetryDecision, DEFAULT_BASE, DEFAULT_CAP, DEFAULT_FACTOR, DEFAULT_MAX_ATTEMPTS,
};
pub use classify::{classify_status, resolve_terminality, Terminality};
pub use deadline::{
    CallClass, DeadlinePolicy, BLOB_STALL, METADATA_TOTAL, SEND_TOTAL, SUBSCRIBE_CONNECT,
};
