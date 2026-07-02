//! The reconciler's hooks into the host's durable outbox (D44).
//!
//! The engine owns *when* reconciliation runs (every connect, first included)
//! and *how* it drives forward — but it does not know IndexedDB. The host
//! supplies two callbacks: the set of never-dispatched forward requests to
//! replay, and a settle-link callback invoked when a replay succeeds. This is
//! how the view-open `resendNeverDispatched` trigger (deleted) becomes a
//! connect-triggered engine concern instead.

use futures_util::future::LocalBoxFuture;
use posthaste_contract_core::MutationReceipt;
use posthaste_contract_core::MutationRequest;

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
}
