//! Shared predictor for coherent links.
//!
//! This crate is the single definition of every named mutation's **local
//! effect** — the pure `apply(state, mutation) -> state` fold that produces a
//! node's optimistic state ([replication L1 §4.3](../replication/L1.md),
//! `single-local-effect`). The bundled authority runtime and the WASM
//! client-layer replica both call it, so there is exactly one predictor across
//! deployments; no node hand-writes a second copy.
//!
//! It is deliberately leaf and portable: `serde` only, no I/O, no native time,
//! filesystem, or database, so it compiles to `wasm32-unknown-unknown`. A
//! caller maps its own record types onto the minimal canonical state here, folds
//! its pending mutations, and maps the result back. Keeping the *effect* (not
//! the record types) shared is what the invariant requires.
//!
//! @spec docs/replication/client-link/L2#1-the-shared-predictor-crate-posthaste-link-core

mod convergence;
mod message;

pub use convergence::{
    MessageBaseUpdate, MessageReplica, MutationId, PendingMessageMutation, SettlementOutcome,
    SettlementResult,
};
pub use message::{
    apply_message_assertion, coalesce_message_assertions, replay_message, KeywordDelta,
    MessageAssertion, MessageChangeDiff, MessageFoldState, MessageOutcome,
};
