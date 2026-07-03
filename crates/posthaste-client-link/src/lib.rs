//! The client↔runtime link ops extracted from `RuntimeCore`
//! (RFC-L2-architecture-cleanup D7/D23): one trait ([`RuntimeLink`]) for the
//! link protocol — link open/close, the three stream families (runtime frames,
//! view snapshots, events), link-view open/extend/close, and `forward_mutation`
//! forward — plus the subscription/stream types those methods return.
//!
//! The four families are one protocol: every consumer (api `runtime_stream` +
//! `sync_events`, testkit `settle`, bench workload) opens a link,
//! opens views, subscribes, and forwards mutations *together*; splitting one
//! protocol across traits is ceremony with no subset consumer (XXI). So this is
//! ONE trait, not several.
//!
//! `replay_events` is **not** here: it had zero production consumers (the runtime
//! calls itself to build the subscription's replay backlog — that logic stays a
//! private fn in `posthaste-runtime`; the public replay rides on
//! [`RuntimeEventSubscription::replay`]). The sessionless `open_view`/
//! `subscribe_view` pair is **not** here either (D51/M10): it had zero call
//! sites post-M9b2 (all views flow link-scoped through
//! [`open_link_view`](RuntimeLink::open_link_view)/[`subscribe_runtime_frames`](RuntimeLink::subscribe_runtime_frames));
//! the D23 verdict that kept it verified wiring, not call sites, and was
//! stale. [`RuntimeViewSubscription`]/[`RuntimeViewFrameStream`] stay: the
//! far-end `ViewRegistry` still uses them for the link-scoped path's internal
//! per-view broadcast.

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use posthaste_contract_core::{
    ClientMutationId, MutationReceipt, MutationRequest, RuntimeCaller, RuntimeError, RuntimeFrame,
    RuntimeLinkConnection, RuntimeLinkId, RuntimeLinkSeq, ViewDescriptor, ViewFrame, ViewId,
    ViewSnapshot,
};
use posthaste_domain_model::{DomainEvent, EventFilter};

/// Live runtime event stream returned by authority runtimes.
pub type RuntimeEventStream = BoxStream<'static, DomainEvent>;

/// Runtime-owned event subscription (RFC-L2-scripting D52 — the fact-carrying
/// tap on `/v1/events`): the durable prelude followed by the live tail. The
/// prelude is, in order, an optional **gap frame** (`gap`) then the replayed
/// facts (`replay`), then `live` streams onward. A gap and a replay can BOTH be
/// present: when the cursor fell before the log's oldest retained seq the gap
/// signals the (possibly-lossy) truncation, but any facts that survive after the
/// cursor are still replayed — a purge must never silently drop a surviving fact.
pub struct RuntimeEventSubscription {
    /// The durable facts replayed after the resume cursor (empty for a fresh
    /// attach).
    pub replay: Vec<DomainEvent>,
    /// `Some(highest_seq)` when the resume opened a **gap frame** (§3): the
    /// consumer's cursor is before the log's oldest retained seq; it adopts
    /// `highest_seq` and continues (deduping the retained `replay` that follows).
    /// The mount emits this as a distinguishable wire element ahead of `replay`.
    pub gap: Option<u64>,
    /// The live tail, resuming durably from the cursor on a broadcast overflow.
    pub live: RuntimeEventStream,
}

pub type RuntimeViewFrameStream = BoxStream<'static, ViewFrame>;
pub type RuntimeFrameStream = BoxStream<'static, RuntimeFrame>;

pub struct RuntimeViewSubscription {
    pub catch_up: Option<ViewFrame>,
    pub live: RuntimeViewFrameStream,
}

pub struct RuntimeFrameSubscription {
    pub catch_up: Vec<RuntimeFrame>,
    pub live: RuntimeFrameStream,
}

/// The client↔runtime link protocol: links, the three stream families
/// (frames / views / events), link-view snapshots, and `forward_mutation`
/// forward. Every method takes `caller: RuntimeCaller` first (shared caller
/// identity, lives in `posthaste-contract-core`).
#[async_trait]
pub trait RuntimeLink: Send + Sync {
    async fn open_link(&self, caller: RuntimeCaller) -> Result<RuntimeLinkConnection, RuntimeError>;

    async fn close_link(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
    ) -> Result<(), RuntimeError>;

    async fn subscribe_runtime_frames(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
        after_seq: Option<RuntimeLinkSeq>,
    ) -> Result<RuntimeFrameSubscription, RuntimeError>;

    async fn open_link_view(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
        descriptor: ViewDescriptor,
    ) -> Result<ViewSnapshot, RuntimeError>;

    async fn close_link_view(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
        view_id: ViewId,
    ) -> Result<(), RuntimeError>;

    /// Grow an open windowed link view by `count` rows, returning the
    /// extended snapshot (also broadcast as a `ViewReplace` frame).
    async fn extend_link_view(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
        view_id: ViewId,
        count: usize,
    ) -> Result<ViewSnapshot, RuntimeError>;

    async fn forward_mutation(
        &self,
        caller: RuntimeCaller,
        request: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError>;

    /// The settlement the runtime holds for one `(link, clientMutationId)`
    /// key, or `None` when it has no record (unknown link, never accepted,
    /// or already evicted/cleared under the D47 ledger rule). The near-end
    /// reconciler's cross-link sent-but-unsettled query (D44b): a terminal
    /// receipt settles locally; `None` re-forwards.
    async fn mutation_settlement(
        &self,
        caller: RuntimeCaller,
        link_id: RuntimeLinkId,
        client_mutation_id: ClientMutationId,
    ) -> Result<Option<MutationReceipt>, RuntimeError>;

    async fn subscribe_events(
        &self,
        caller: RuntimeCaller,
        filter: EventFilter,
    ) -> Result<RuntimeEventSubscription, RuntimeError>;
}
