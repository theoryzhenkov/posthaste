//! Client-layer reactive entity store.
//!
//! Wraps the [`posthaste_replica_core`] convergence engine with a normalized,
//! keyed entity store ([client-link L2 §2](../replication/client-link/L2.md)):
//! `message[id]` and `view[viewId]` (an ordered row list + coverage). The host
//! feeds it authoritative batches — message mutations carrying the full
//! projection — and the store applies the batch atomically, self-maintains
//! each evaluable view's membership, then reports the changed keys. Optimism is
//! a pure fold over the confirmed base (the shared predictor); the store never
//! stores it as truth. Mailbox counts are NOT held here: the client reads them
//! via react-query invalidation of the runtime's canonical counts
//! (RFC-L2-count-unification).
//!
//! Layered per RFC D36: [`mechanism`] is the accept/settle/retire plumbing
//! over replica-core's `OptimisticReplica` kernel (layer 1 mount); [`projection`]
//! is the keyed view rows / predicates / windowing over it (layer 2, the
//! shared projector of D38); [`entity_store::EntityStore`] is the public
//! composition of both. A headless client consumes exactly these layers.
//!
//! Like `posthaste-replica-core` this is portable (serde only, no I/O): transport +
//! persistence belong to the host (the web adapter).
//!
//! @spec docs/replication/client-link/L2#2-the-replica-node-posthaste-replica-projector
//! @spec docs/eph/DESIGN-L2-client-link-reactive-store

pub mod entity_store;
pub mod mechanism;
pub mod projection;

pub use entity_store::{EntityStore, StoreUpdate};
pub use mechanism::{apply_fold_to_projection, fold_state_from_projection, project_optimistic};
pub use projection::{DirtyKey, SortDirection, SortKey, ViewEntity, ViewPredicate, ViewRow};
