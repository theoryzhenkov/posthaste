//! Client-layer reactive entity store.
//!
//! Wraps the [`posthaste_link_core`] convergence engine with a normalized,
//! keyed entity store ([client-link L2 §2](../replication/client-link/L2.md)):
//! `message[id]`, `mailbox[id]` (server-authoritative count scalars), and
//! `view[viewId]` (an ordered row list + coverage). The host feeds it
//! authoritative batches — message mutations (carrying the full projection) +
//! count deltas — and the store applies the batch atomically, self-maintains
//! each evaluable view's membership, then reports the changed keys. Optimism is
//! a pure fold over the confirmed base (the shared predictor); the store never
//! stores it as truth.
//!
//! Like `posthaste-link-core` this is portable (serde only, no I/O): transport +
//! persistence belong to the host (the web adapter).
//!
//! @spec docs/replication/client-link/L2#2-the-replica-node-posthaste-link-replica
//! @spec docs/eph/DESIGN-L2-client-link-reactive-store

pub mod entity_store;

pub use entity_store::{
    apply_fold_to_projection, fold_state_from_projection, CountDelta, DirtyKey, EntityStore,
    MailboxEntity, SortDirection, SortKey, StoreUpdate, ViewEntity, ViewPredicate, ViewRow,
};
