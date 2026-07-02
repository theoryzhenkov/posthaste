//! The reconciler's hooks into the host's durable outbox (D44).
//!
//! The engine owns *when* reconciliation runs (every connect, first included)
//! and *how* it drives forward — but it does not know IndexedDB. The host
//! supplies the record sets to reconcile and the settle/link callbacks the
//! reconciler invokes. This is how the view-open `resendNeverDispatched`
//! trigger (deleted) and the sent-but-unsettled TODO both become
//! connect-triggered engine concerns instead.

use futures_util::future::LocalBoxFuture;
use posthaste_contract_core::MutationReceipt;
use posthaste_contract_core::MutationRequest;

/// A record the host sent (holds a `runtimeMutationId`) but never saw settle
/// terminally — the session-continuity-loss case the reconciler resolves via
/// the wire's settlement query (D44b).
#[derive(Clone, Debug)]
pub struct SentUnsettled {
    /// The session the record was dispatched under (the settlement query is
    /// keyed to it — a later session cannot see another session's ledger).
    pub session_id: String,
    pub client_mutation_id: String,
    /// The original forward request, for the re-forward path when the runtime
    /// has no record. `None` when the host cannot reconstruct it (the record is
    /// then left alone).
    pub request: Option<MutationRequest>,
}

/// Host callbacks for the level-triggered reconciler. Object-safe: held as
/// `Rc<dyn OutboxHooks>`.
pub trait OutboxHooks {
    /// The forward requests the host optimistically accepted but has **no**
    /// evidence reached the runtime (no linked runtime-mutation id). The
    /// reconciler replays each on connect — safe because never-dispatched means
    /// no server-side application, and the runtime dedups a same-session
    /// re-forward by `clientMutationId`.
    fn never_dispatched(&self) -> LocalBoxFuture<'static, Vec<MutationRequest>>;

    /// A replayed forward returned `receipt`: the host links its
    /// `runtimeMutationId` so the record is no longer never-dispatched (and a
    /// later terminal settlement can retire it). Failures on the replay path are
    /// surfaced through the normal frame/settlement stream, not here.
    fn on_reconciled(&self, receipt: MutationReceipt) -> LocalBoxFuture<'static, ()>;

    /// The records the host dispatched (receipt held) but never saw settle
    /// terminally. Queried against the runtime on every connect (D44b) when the
    /// wire has a settlement query.
    fn sent_unsettled(&self) -> LocalBoxFuture<'static, Vec<SentUnsettled>>;

    /// The settlement query found a terminal verdict for a sent-but-unsettled
    /// record: the host settles it locally (retire/revert the optimism, clear
    /// the durable record).
    fn on_settlement(&self, receipt: MutationReceipt) -> LocalBoxFuture<'static, ()>;
}
