//! The native outbound-call **envelope** (RFC-L2-provider-reliability D80/D83, M31).
//!
//! This crate is the *executor* half over the wasm-pure
//! [`posthaste_call_policy`] core: where the policy crate decides *how* a call
//! retries/deadlines/classifies as pure arithmetic, [`ProviderCallExecutor`]
//! *runs* it on tokio + reqwest. It closes the JMAP engine's audited network-edge
//! defects at one seam:
//!
//! * **F1** (no retry/429 anywhere) — a `Retry-After`-aware jittered retry loop
//!   that retries only [`Terminality`](posthaste_call_policy::Terminality)
//!   `Transient` outcomes.
//! * **F2** (10 s total-timeout monoculture) — per-class deadlines from
//!   [`CallClass::deadline_policy`](posthaste_call_policy::CallClass): a *total*
//!   for metadata/send, a between-chunks *stall* read-deadline for blobs (via the
//!   [`stall_guard`] stream adapter), so a large-but-progressing download
//!   completes instead of dying at 10 s forever.
//! * **F3** (failure-class collapse) — outcomes classified once at the edge over
//!   the shared terminality taxonomy (D82).
//! * **F4** (a fresh client per mutation) — one shared `reqwest::Client`
//!   (connection pool) per executor.
//!
//! It also adds the **per-account circuit breaker** (D83): after
//! [`BREAKER_FAILURE_THRESHOLD`] consecutive failures a per-account breaker opens
//! for [`BREAKER_COOLDOWN`], fast-failing calls with a distinct
//! [`CallErrorReason::CircuitOpen`] reason and admitting a single half-open probe
//! after cooldown — never global (R86).
//!
//! Native-only by construction (tokio + reqwest): nothing on the D15 wasm
//! frontier depends on it.

mod breaker;
mod error;
mod executor;
mod retry_after;
mod stall;

pub use breaker::{BreakerConfig, BreakerPhaseView, BREAKER_COOLDOWN, BREAKER_FAILURE_THRESHOLD};
pub use error::{CallErrorReason, ProviderCallError};
pub use executor::{ExecutorConfig, HttpRequestSpec, ProviderCallExecutor, ProviderResponse};
pub use stall::{stall_guard, StallError};

// Re-export the policy vocabulary callers need so a downstream crate can route a
// call without also depending on `posthaste-call-policy` directly.
pub use posthaste_call_policy::{
    BackoffSchedule, CallClass, BLOB_STALL, METADATA_TOTAL, SEND_TOTAL,
};

#[cfg(test)]
mod tests;
