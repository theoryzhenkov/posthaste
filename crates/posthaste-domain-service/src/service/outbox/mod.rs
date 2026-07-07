//! Tier-2 (runtime <-> provider) outbox engine: enqueue and flush.
//!
//! Callers enqueue an [`Operation`]; pending operations form a read-time overlay
//! and the flusher drains them to the provider, settling applied/failed outcomes
//! and emitting `operation.settled` events. Draft ops carry the STABLE draft key
//! as their entity id and resolve it to the current live id at push time via the
//! `DraftRegistry` (M70/D136); a provider-assigned rotation is recorded as one
//! registry repoint at settlement.
//!
//! Decomposed by concern:
//! - [`queue`]: enqueue/list/discard/retry + assertion coalescing.
//! - [`draft`]: draft save/delete/discard + flush-time identity resolution.
//! - [`flush`]: the drain loop (`flush_account` / `flush_pass`).
//! - [`push`]: per-`OperationKind` provider dispatch.
//! - [`classify`]: gateway-error -> flush-disposition classification.
//! - [`settle`]: readback settlement + settlement/uncertainty event emission.
//! - [`schedule`]: scheduled-send (`send_at`) normalization + the
//!   monotonic-anchored due clock (undo-send / send-later ride one hold).
//!
//! @spec docs/L1-outbox#operation-model
//! @spec docs/L1-outbox#state-machine

mod classify;
mod draft;
mod flush;
mod push;
mod queue;
pub(crate) mod schedule;
mod settle;
