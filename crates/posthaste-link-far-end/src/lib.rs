//! The link **far-end** engine (RFC D40/D45/D52): the composable native
//! sub-stores a fan-in node's serving half assembles, factored into its two
//! channel halves.
//!
//! A link's far-end is the serving half a node mounts only because it fans in
//! downstream links ([replication L1 §10](../replication/L1.md), D39). D40 (as
//! amended by Q7) makes it composable **sub-stores** each far-end assembles —
//! not one struct-per-subscriber. D52 splits those sub-stores into two halves so
//! a **tap** (a read-only far-end) can mount the down half alone:
//!
//! - [`down`] — the outbound stream path: the seq-backlog [`ReplayStore`], the
//!   [`Sequenced`]/`Reset` wire envelope, the durable [`FactLog`] port, and the
//!   fact-carrying [`Tap`] with its subscriber registry + TTL reaper.
//! - [`up`] — the inbound mutation path: the [`DedupStore`] idempotency ledger
//!   and the [`SettlementSinkStore`] settlement routing.
//!
//! The stores are generic over each seam's `LinkId` (concrete ids stay in the
//! seam crates); both the runtime's far-end (client↔runtime seam) and the
//! authority server's far-end (runtime↔authority-server seam) assemble the same
//! sub-stores. A tap ([`Tap`]) assembles the down half only.
//!
//! [`ReplayStore`]: down::ReplayStore
//! [`Sequenced`]: down::Sequenced
//! [`FactLog`]: down::FactLog
//! [`Tap`]: down::Tap
//! [`DedupStore`]: up::DedupStore
//! [`SettlementSinkStore`]: up::SettlementSinkStore

pub mod down;
pub mod up;
