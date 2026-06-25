//! Client-layer replica view layer.
//!
//! Wraps the [`posthaste_link_core`] convergence engine with the working-set
//! view logic ([client-link L2 §2](../replication/client-link/L2.md)): it takes the
//! runtime's served mail-list rows as its confirmed base, folds the outbox of
//! pending mutations over them with the shared predictor, and serves optimistic
//! rows to the renderer — so an archive/flag/read shows instantly instead of
//! after a round-trip to a remote runtime.
//!
//! Like `posthaste-link-core` this is portable (serde only, no I/O): the
//! contract↔replica mapping, transport, and persistence belong to the host (the
//! web adapter or native node). General query-membership re-evaluation (does a
//! folded row still match a smart-mailbox?) is injected as a predicate and
//! otherwise deferred to the runtime's authoritative recompute — that is the
//! later coverage/atoms layer, not this one.
//!
//! @spec docs/replication/client-link/L2#2-the-replica-node-posthaste-link-replica

mod mail_list;

pub use mail_list::{MailListReplica, MailListRow};
