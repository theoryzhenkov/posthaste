//! The link **far-end** engine (RFC D40/D45): the composable native sub-stores
//! a fan-in node's serving half assembles.
//!
//! A link's far-end is the serving half a node mounts only because it fans in
//! downstream links ([replication L1 §10](../replication/L1.md), D39). D40 (as
//! amended by Q7) makes it three composable **sub-stores** each far-end
//! assembles — not one struct-per-subscriber:
//!
//! - [`dedup`] — the idempotency ledger, keyed `(LinkId, ClientMutationId)`,
//!   with the D47 terminal-class keep/clear rule.
//! - [`sink`] — per-`LinkId` settlement-to-originator routing + subscriber
//!   registration + a tick-driven expiry reaper.
//! - [`replay`] — the seq-backlog: monotonic per-subscriber seq, a bounded
//!   backlog buffer, resume-from-`after_seq`, and the collapse-to-current-state
//!   fallback signal (D46).
//!
//! The stores are generic over each seam's `LinkId` (concrete ids stay in the
//! seam crates); both the runtime's far-end (client↔runtime seam) and the
//! authority server's far-end (runtime↔authority-server seam) assemble the same
//! sub-stores.

pub mod dedup;
pub mod replay;
pub mod sink;

pub use dedup::{Accept, DedupStore, TerminalClass, DEFAULT_TERMINAL_CAPACITY};
pub use replay::{ReplayStore, Resume, Sequenced, DEFAULT_BACKLOG_CAPACITY};
pub use sink::{SettlementSinkStore, DEFAULT_SINK_TTL};
