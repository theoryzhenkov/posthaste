//! The client↔runtime link ops extracted from `RuntimeCore`
//! (RFC-L2-architecture-cleanup D7/D23): one trait ([`RuntimeLink`]) for the
//! link protocol — link open/close, the three stream families (runtime frames,
//! view snapshots, events), link-view open/extend/close, and `forward_mutation`
//! forward — plus the subscription/stream types those methods return.
//!
//! The four families are one protocol: every consumer (api `runtime_stream` +
//! `views` + `sync_events`, testkit `settle`, bench workload) opens a link,
//! opens views, subscribes, and forwards mutations *together*; splitting one
//! protocol across traits is ceremony with no subset consumer (XXI). So this is
//! ONE trait, not several.
//!
//! `replay_events` is **not** here: it had zero production consumers (the runtime
//! calls itself to build the subscription's replay backlog — that logic stays a
//! private fn in `posthaste-runtime`; the public replay rides on
//! [`RuntimeEventSubscription::replay`]). The sessionless `open_view`/
//! `subscribe_view` pair **is** here: `posthaste-http-api-adapter`'s `POST /v1/views` and
//! `GET /v1/views/{id}/stream` routes are live production consumers.

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use posthaste_contract_core::{
    ClientMutationId, MutationReceipt, MutationRequest, RuntimeCaller, RuntimeError, RuntimeFrame,
    RuntimeLinkConnection, RuntimeLinkId, RuntimeLinkSeq, ViewDescriptor, ViewFrame, ViewId,
    ViewRevision, ViewSnapshot,
};
use posthaste_domain_model::{DomainEvent, EventFilter};

/// Live runtime event stream returned by authority runtimes.
pub type RuntimeEventStream = BoxStream<'static, DomainEvent>;

/// Runtime-owned event subscription: optional replayed backlog followed by live events.
pub struct RuntimeEventSubscription {
    pub replay: Vec<DomainEvent>,
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

    async fn open_view(
        &self,
        caller: RuntimeCaller,
        descriptor: ViewDescriptor,
    ) -> Result<ViewSnapshot, RuntimeError>;

    async fn subscribe_view(
        &self,
        caller: RuntimeCaller,
        view_id: ViewId,
        after_revision: Option<ViewRevision>,
    ) -> Result<RuntimeViewSubscription, RuntimeError>;

    async fn subscribe_events(
        &self,
        caller: RuntimeCaller,
        filter: EventFilter,
    ) -> Result<RuntimeEventSubscription, RuntimeError>;
}
