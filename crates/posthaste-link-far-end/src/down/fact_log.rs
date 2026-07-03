//! The durable **fact-log port** (RFC-L2-scripting D52 / §3).
//!
//! A fact-carrying channel (the third channel kind, §3) recovers by **durable
//! replay**: for facts, history IS the payload, so a collapse-to-current-state
//! is data loss. The tap ([`super::tap::Tap`]) therefore replays from a durable,
//! seq-addressed log rather than the in-memory [`super::replay::ReplayStore`]
//! backlog. `FactLog` is that log as a **named per-component responsibility**
//! (D52): not an incidental table each component grows, but the one port each
//! backs with its existing durable event store —
//!
//! - the **runtime** backs it with its `event_log` machinery (`ReadCache::
//!   replay_events` over the store's `event_log` table); bound in S1.
//! - the **authority server** backs it with its own store events; bound in S3.
//!
//! The port carries the four operations a durable, seq-addressed log needs to
//! serve resumable fact replay with an explicit gap signal:
//!
//! - [`append`](FactLog::append) — persist a fact, returning its assigned seq.
//! - [`replay`](FactLog::replay) — every retained fact after a cursor (with an
//!   optional seam filter), in seq order.
//! - [`highest_seq`](FactLog::highest_seq) — the newest seq (the live cursor a
//!   fresh subscriber attaches at, §5.3).
//! - [`truncation_point`](FactLog::truncation_point) — the oldest seq still
//!   retained; a resume from before it cannot be served from durable history, so
//!   the tap emits the **gap frame** instead of silently dropping (§3, N8).

use async_trait::async_trait;

use crate::down::replay::Sequenced;

/// A failure reading or writing the durable fact log. The port stays
/// transport-free (D46), so it carries a seam-opaque message rather than a
/// concrete backing error; the binding maps its store/link error into this.
#[derive(Clone, Debug)]
pub enum FactLogError {
    /// The backing store failed (I/O, query, serialization).
    Backing(String),
    /// This binding is a **read-only** view of a log another component authors
    /// (D52: the tap is a read-only far-end). The runtime tap replays and tails
    /// the authority-authored `event_log` but never appends to it — appends are
    /// the authoring component's write path.
    ReadOnly,
}

impl std::fmt::Display for FactLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backing(message) => write!(f, "fact log backing error: {message}"),
            Self::ReadOnly => write!(f, "fact log is read-only for this binding"),
        }
    }
}

impl std::error::Error for FactLogError {}

/// A durable, seq-addressed log of facts a fact-carrying tap replays from (D52).
///
/// Implementors are the per-component backings (runtime `event_log`; authority
/// server store). The trait is generic over the seam's `Fact` (the runtime's
/// `DomainEvent`) and an optional `Filter` (the runtime's `EventFilter`) so the
/// port stays seam-agnostic while a concrete binding narrows both.
#[async_trait]
pub trait FactLog: Send + Sync {
    /// The fact this log carries (the runtime's `DomainEvent`).
    type Fact: Clone + Send + 'static;
    /// A seam filter composed with the resume cursor on [`replay`](Self::replay)
    /// (the runtime's `EventFilter` — topic/account/mailbox scope). `()` for a
    /// log that serves the whole stream unfiltered.
    type Filter: Send + Sync;

    /// Append `fact` durably and return its assigned monotonic seq. A read-only
    /// binding (the runtime tap over the authority-authored log) returns
    /// [`FactLogError::ReadOnly`] — its facts are appended by the authoring
    /// component's write path, not through the tap.
    async fn append(&self, fact: Self::Fact) -> Result<u64, FactLogError>;

    /// Every retained fact with `seq > after_seq`, in ascending seq order, each
    /// wrapped in its [`Sequenced::Frame`] envelope. `filter` narrows the stream
    /// to the subscriber's authz/topic scope (composed with the cursor). The
    /// caller first checks [`truncation_point`](Self::truncation_point): a resume
    /// from before it is a gap, not an empty replay.
    async fn replay(
        &self,
        after_seq: u64,
        filter: Option<Self::Filter>,
    ) -> Result<Vec<Sequenced<Self::Fact>>, FactLogError>;

    /// The highest seq the log has assigned, or `0` when empty — the live cursor
    /// a fresh subscriber attaches at (§5.3 snapshot-attach).
    async fn highest_seq(&self) -> Result<u64, FactLogError>;

    /// The oldest seq still retained (the truncation point). A resume whose
    /// cursor is before it (`after_seq + 1 < truncation_point`) cannot be served
    /// from durable history, so the tap emits the gap frame (§3). `0` when the
    /// log is empty or has never been truncated from the very first seq.
    async fn truncation_point(&self) -> Result<u64, FactLogError>;
}
