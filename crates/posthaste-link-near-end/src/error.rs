//! Permanent-vs-transient classification — one rule, over the *shared*
//! vocabulary, now owned by the shared policy core.
//!
//! The TS client split this fact three ways (`FatalStreamError` on 4xx in
//! `httpAdapter`, silent transient elsewhere, nothing on the forward path). The
//! rule is owned once, and it is owned in the workspace-wide type: the verdict
//! is [`Terminality`] (the D82 taxonomy, O2-ruled to live in
//! `posthaste-domain-model`), the same enum the outbox flush and the D47
//! settlement seam speak.
//!
//! Post-M30 the classification *arithmetic* is the shared
//! [`posthaste_call_policy`] policy core (D80/D82): [`classify_status`] is its
//! HTTP status-band table (4xx permanent, everything else transient) and
//! `EngineError::from_response` applies its `resolve_terminality` precedence rule
//! (envelope wins when present, status band is the fallback). This module is now
//! the near-end's thin re-export of that shared vocabulary.

pub use posthaste_call_policy::{classify_status, Terminality};
