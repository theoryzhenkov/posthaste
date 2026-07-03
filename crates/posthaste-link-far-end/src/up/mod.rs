//! The far-end's **up-channel** half (RFC D52): the sub-stores a serving node
//! assembles for the *inbound* mutation path — client/runtime writes flowing up
//! toward the authority.
//!
//! - [`dedup`] — the idempotency ledger, keyed `(LinkId, ClientMutationId)`,
//!   with the D47 terminal-class keep/clear rule and D48 time-and-ack retention.
//! - [`sink`] — per-`LinkId` settlement-to-originator routing + a tick-driven
//!   expiry reaper.
//!
//! A **tap** ([`crate::down::tap`]) mounts the down half alone and never touches
//! this half — a read-only consumer has no writes to dedup and no settlements to
//! route (D52; the stateless consumer contract §5, sinkless).

pub mod dedup;
pub mod sink;

pub use dedup::{
    Accept, DedupStore, TerminalClass, DEFAULT_TERMINAL_CAPACITY, DEFAULT_TERMINAL_TTL,
};
pub use sink::{SettlementSinkStore, DEFAULT_SINK_TTL};
