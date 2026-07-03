//! The far-end's **down-channel** half (RFC D52): the machinery a serving node
//! assembles for the *outbound* stream path — frames flowing down toward a
//! subscriber.
//!
//! - [`replay`] — the seq-backlog: a monotonic per-subscriber seq, a bounded
//!   backlog buffer, resume-from-`after_seq`, and the [`Sequenced`]/`Reset`
//!   down-wire envelope (D46/D49).
//! - [`fact_log`] — the [`FactLog`] port (D52): a durable, seq-addressed fact
//!   log a fact-carrying tap replays from (the runtime backs it with its
//!   `event_log`; the authority server with its store — S3).
//! - [`tap`] — [`Tap`] (D52): the down half instantiated alone, replaying a
//!   durable [`FactLog`] instead of an in-memory backlog, with a subscriber
//!   registry + TTL reaper and no up-half. A resume past the log's truncation
//!   point yields the explicit **gap frame** (the `Reset` element reinterpreted
//!   per §3 — never silent, never collapse).

pub mod fact_log;
pub mod replay;
pub mod tap;

pub use fact_log::{FactLog, FactLogError};
pub use replay::{ReplayStore, Resume, Sequenced, DEFAULT_BACKLOG_CAPACITY};
pub use tap::{Tap, TapResume, DEFAULT_TAP_TTL};
