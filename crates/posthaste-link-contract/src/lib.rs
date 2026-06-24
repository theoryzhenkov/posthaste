//! The shared coherent-link contract — the replication subset both links speak.
//!
//! A coherent link ([replication L1 §2](../replication/L1.md)) carries exactly
//! two channels between a near node and a far node: named mutations forwarded
//! **up**, authoritative base assertions + per-mutation confirmation streamed
//! **down**. This crate is the single, transport-neutral definition of those two
//! channels — *not* the full [`RuntimeCore`] surface, only its replication
//! subset ([replication L4 §3-§4](../replication/L4.md)).
//!
//! Both seams use it. The client↔runtime link already speaks this vocabulary on
//! the wire (it `POST`s [`MutationRequest`] → [`MutationReceipt`] and streams
//! frames), so it is **conformant by construction** — the contract is factored
//! *from* it, not invented beside it (§4.1). The runtime↔backend link adopts the
//! same shape via [`BackendLink`].
//!
//! [`BackendApi`] is the Rust abstraction over one link's two channels; it is
//! selected by configuration (in-process co-located by default, remote when
//! split — [replication L4 §5](../replication/L4.md)). The transport is what
//! varies across deployments; the contract above it does not. This is the seam
//! the `one-link-transport` assertion guards: one shared contract + one Rust
//! transport abstraction, never a second bespoke mechanism.
//!
//! @spec docs/replication/L4#3-the-link-contract-backendlink
//! @spec docs/replication/L4#4-the-transport-abstraction-one-seam-for-both-links

use std::sync::Arc;

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use serde::{Deserialize, Serialize};

use posthaste_domain::{
    AccountId, ConversationId, ConversationView, MessageDetail, MessageId, MessageSummary,
};
use posthaste_link_core::{MessageFoldState, MutationId, SettlementOutcome};
use posthaste_runtime_contract::{
    MailQueryPage, MailQueryRequest, MutationReceipt, MutationRequest, RuntimeError,
    RuntimeErrorCode,
};

/// Wire path for the link up-channel: a remote near node `POST`s a
/// [`MutationRequest`] (JSON) here and receives a [`MutationReceipt`]. Shared by
/// the remote transport client and the far-node HTTP surface so the two cannot
/// drift ([replication L4 §4](../replication/L4.md)).
pub const LINK_FORWARD_MUTATION_PATH: &str = "/v1/link/mutations";

/// Wire path for the link down-channel: a remote near node opens an SSE stream
/// here whose `data:` frames are JSON [`DownFrame`]s.
pub const LINK_SUBSCRIBE_PATH: &str = "/v1/link/subscribe";

/// Wire path for the read channel's mail-list query: a remote near node `POST`s
/// a [`MailQueryRequest`] and receives a [`MailQueryPage`]. The query engine
/// runs at the far node (the authority); the near node reads through to it.
pub const LINK_QUERY_PATH: &str = "/v1/link/query";

/// Wire path for the read channel's point read: the current [`MessageSummary`]
/// of one message (the read behind undo-history). `POST`ed as `{accountId,
/// messageId}`, returns the summary or null.
pub const LINK_SUMMARY_PATH: &str = "/v1/link/summary";

/// Wire path for the read channel's message detail (the `messageDetail` view).
/// `POST`ed as `{accountId, messageId}`, returns the detail or null.
pub const LINK_DETAIL_PATH: &str = "/v1/link/detail";

/// Wire path for the read channel's conversation (the `conversation` view).
/// `POST`ed as `{conversationId}`, returns the folded conversation.
pub const LINK_CONVERSATION_PATH: &str = "/v1/link/conversation";

/// One authoritative base update for a single message, carried on the
/// down-channel ([replication L1 §5.1](../replication/L1.md)). The near node
/// rebases its base cache on each: a new asserted confirmed state, or a removal.
///
/// This is the wire-shaped, serializable twin of `link_core::MessageBaseUpdate`
/// (which is internal to the convergence engine and not `Serialize`). The near
/// node maps between the two when applying a frame to its `MessageReplica`
/// (W2); keeping the wire type here lets the remote transport (W3) serialize it
/// without leaking the engine's internal enum.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BaseUpdate {
    /// The message's confirmed canonical state is now this.
    Present(MessageFoldState),
    /// The message left the served base (authoritative removal).
    Removed,
}

/// An authoritative before/after state assertion over one message
/// ([replication L4 §3](../replication/L4.md)). Ordered within a frame; the near
/// node applies them to its base cache in order, then recomputes its derived
/// views (never invalidates).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseAssertion {
    pub message_id: String,
    pub update: BaseUpdate,
}

/// One frame on the link's down-channel ([replication L4 §3](../replication/L4.md)).
///
/// The "confirmation watermark" (how far the far node has confirmed the near
/// node's forwarded mutations) is realized **per mutation** as
/// [`DownFrame::Settlement`] — the shape the contract already serves on the
/// client↔runtime wire (`RuntimeFrame::MutationSettlement`) — rather than as a
/// scalar high-water mark. By the state-before-event rule the matching base
/// assertion arrives first, so a confirmed settlement is a visual no-op; a
/// failed one drives the near node's recompute back to authoritative state
/// ([replication L1 §5.3, §5.5](../replication/L1.md)).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DownFrame {
    /// A batch of ordered authoritative base updates to apply to the base cache.
    Base { assertions: Vec<BaseAssertion> },
    /// A forwarded mutation reached its terminal outcome at the far node — the
    /// per-mutation confirmation watermark. Retires the matching outbox entry.
    Settlement {
        mutation_id: WireMutationId,
        outcome: WireSettlementOutcome,
    },
    /// Liveness only; carries no state. Lets a remote transport keep the
    /// down-stream open without implying a base change.
    Heartbeat,
}

/// Serializable mirror of [`MutationId`] for the wire (the engine's `MutationId`
/// is a plain newtype but lives in `link-core`; re-exposing the wire form here
/// keeps the contract self-contained).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WireMutationId(pub String);

impl WireMutationId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<MutationId> for WireMutationId {
    fn from(id: MutationId) -> Self {
        Self(id.0)
    }
}

impl From<WireMutationId> for MutationId {
    fn from(id: WireMutationId) -> Self {
        Self(id.0)
    }
}

/// Serializable mirror of [`SettlementOutcome`] for the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WireSettlementOutcome {
    Confirmed,
    Failed,
}

impl From<SettlementOutcome> for WireSettlementOutcome {
    fn from(outcome: SettlementOutcome) -> Self {
        match outcome {
            SettlementOutcome::Confirmed => Self::Confirmed,
            SettlementOutcome::Failed => Self::Failed,
        }
    }
}

impl From<WireSettlementOutcome> for SettlementOutcome {
    fn from(outcome: WireSettlementOutcome) -> Self {
        match outcome {
            WireSettlementOutcome::Confirmed => SettlementOutcome::Confirmed,
            WireSettlementOutcome::Failed => SettlementOutcome::Failed,
        }
    }
}

/// What slice of the far node's base the near node subscribes to
/// ([replication L4 §3](../replication/L4.md)). The co-located runtime requests
/// [`Complete`](LinkCoverage::Complete) coverage (it serves the whole working
/// set); a split runtime may request a [`WorkingSet`](LinkCoverage::WorkingSet)
/// so it can distinguish "absent because unchanged" from "absent because not
/// held". The working-set shape is left open for the split-runtime slice (W4);
/// today only `Complete` is exercised.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LinkCoverage {
    /// The far node serves its complete base down — the co-located default.
    #[default]
    Complete,
    /// The far node serves only a working set. `descriptor` names it; its shape
    /// is defined when split runtimes land (W4).
    WorkingSet {
        #[serde(default)]
        descriptor: serde_json::Value,
    },
}

/// The ordered down-channel: authoritative base assertions + confirmation,
/// tagged with the watermark per [`DownFrame`].
pub type DownStream = BoxStream<'static, DownFrame>;

/// One link's two channels, transport-neutral. The transport is the only thing
/// that varies across deployments — in-process and co-located by default
/// (W1, behavior-preserving), remote when the far node lives elsewhere
/// (W3) — and is selected by configuration, never at build time
/// ([replication L4 §5](../replication/L4.md), assertion `transport-selected-by-config`).
///
/// The same trait carries **both** links (assertion `one-link-transport`): the
/// client↔runtime link is conformant by construction (the contract is the wire
/// it already speaks), and the runtime↔backend link adopts it via
/// [`BackendLink`].
#[async_trait]
pub trait BackendApi: Send + Sync {
    /// Up-channel. Forward a (possibly client-originated) named mutation toward
    /// the far node with a stable mutation id; the receipt carries the far
    /// node's `RuntimeMutationId` for the confirmation join. Idempotent on the
    /// mutation id ([replication L4 §3](../replication/L4.md)).
    async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError>;

    /// Down-channel. Subscribe to the far node's ordered stream of base
    /// assertions + per-mutation confirmation for a coverage. The near node
    /// rebases its base cache on each frame and recomputes its derived views.
    async fn subscribe(&self, coverage: LinkCoverage) -> Result<DownStream, RuntimeError>;

    /// Read channel: compute a page of a mail-list query at the far node (the
    /// query engine is the authority's, [`DESIGN-L4-read-replication`](../../eph/DESIGN-L4-read-replication.md)).
    /// A near node reads through here on a cache miss. The default errors: a
    /// transport that does not carry the read channel (e.g. a write-only test
    /// stub) is simply not a read source.
    async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        let _ = request;
        Err(read_channel_unsupported())
    }

    /// Read channel: the current canonical summary of one message (the point
    /// read behind undo-history). `None` when the far node does not hold it.
    async fn current_summary(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError> {
        let _ = (account_id, message_id);
        Err(read_channel_unsupported())
    }

    /// Read channel: a message's detail (header + attachments) for the
    /// `messageDetail` view.
    async fn message_detail(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        let _ = (account_id, message_id);
        Err(read_channel_unsupported())
    }

    /// Read channel: an overlay-folded conversation for the `conversation` view.
    async fn conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        let _ = conversation_id;
        Err(read_channel_unsupported())
    }
}

fn read_channel_unsupported() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorCode::Internal,
        "link transport does not carry the read channel",
    )
}

/// The runtime↔backend link ([replication L4 §3](../replication/L4.md)): the
/// runtime's typed handle to the backend, carried by a swappable
/// [`BackendApi`]. The runtime reaches the backend **only** through these two
/// channels — never by reading the backend store across the link (assertion
/// `backend-link-is-replication-only`); reads become state the near node derives
/// locally from its base cache (W2).
///
/// This is the runtime↔backend *instantiation* of the shared contract. The
/// client↔runtime link is the same contract carried by the same transport
/// abstraction, so there is one mechanism, two consumers.
#[derive(Clone)]
pub struct BackendLink {
    transport: Arc<dyn BackendApi>,
}

impl BackendLink {
    /// Build a backend link over a transport. The transport is config-selected
    /// upstream ([replication L4 §5](../replication/L4.md)); this type does not
    /// know or care which one it holds.
    pub fn new(transport: Arc<dyn BackendApi>) -> Self {
        Self { transport }
    }

    /// Forward a named mutation up to the backend (up-channel).
    pub async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        self.transport.forward_mutation(mutation).await
    }

    /// Subscribe to the backend's authoritative base-assertion stream
    /// (down-channel).
    pub async fn subscribe(&self, coverage: LinkCoverage) -> Result<DownStream, RuntimeError> {
        self.transport.subscribe(coverage).await
    }

    /// Read channel: read a mail-list query page through to the backend (the
    /// authority owns the query engine). A near node reads through here on a
    /// cache miss.
    pub async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.transport.query_mail_page(request).await
    }

    /// Read channel: the current summary of one message through to the backend.
    pub async fn current_summary(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError> {
        self.transport.current_summary(account_id, message_id).await
    }

    /// Read channel: a message's detail through to the backend.
    pub async fn message_detail(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        self.transport.message_detail(account_id, message_id).await
    }

    /// Read channel: a conversation through to the backend.
    pub async fn conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        self.transport.conversation(conversation_id).await
    }

    /// The underlying transport, for callers that need to inspect or hold it.
    pub fn transport(&self) -> &Arc<dyn BackendApi> {
        &self.transport
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_frame_base_round_trips_through_json() {
        let frame = DownFrame::Base {
            assertions: vec![
                BaseAssertion {
                    message_id: "m1".into(),
                    update: BaseUpdate::Present(MessageFoldState {
                        keywords: vec!["$flagged".into()],
                        mailbox_ids: vec!["inbox".into()],
                    }),
                },
                BaseAssertion {
                    message_id: "m2".into(),
                    update: BaseUpdate::Removed,
                },
            ],
        };
        let json = serde_json::to_value(&frame).expect("serialize");
        assert_eq!(json["type"], "base");
        assert_eq!(json["assertions"][0]["update"]["kind"], "present");
        assert_eq!(json["assertions"][1]["update"]["kind"], "removed");
        let restored: DownFrame = serde_json::from_value(json).expect("deserialize");
        assert_eq!(restored, frame);
    }

    #[test]
    fn down_frame_settlement_carries_the_per_mutation_watermark() {
        let frame = DownFrame::Settlement {
            mutation_id: WireMutationId("op1".into()),
            outcome: WireSettlementOutcome::Confirmed,
        };
        let json = serde_json::to_value(&frame).expect("serialize");
        assert_eq!(json["type"], "settlement");
        assert_eq!(json["mutationId"], "op1");
        assert_eq!(json["outcome"], "confirmed");
        let restored: DownFrame = serde_json::from_value(json).expect("deserialize");
        assert_eq!(restored, frame);
    }

    #[test]
    fn wire_mutation_id_and_outcome_bridge_the_engine_types() {
        let engine = MutationId("op7".into());
        let wire: WireMutationId = engine.clone().into();
        assert_eq!(wire.as_str(), "op7");
        assert_eq!(MutationId::from(wire), engine);

        assert_eq!(
            SettlementOutcome::from(WireSettlementOutcome::Failed),
            SettlementOutcome::Failed
        );
        assert_eq!(
            WireSettlementOutcome::from(SettlementOutcome::Confirmed),
            WireSettlementOutcome::Confirmed
        );
    }

    #[test]
    fn link_coverage_defaults_to_complete() {
        assert_eq!(LinkCoverage::default(), LinkCoverage::Complete);
        let json = serde_json::to_value(LinkCoverage::Complete).expect("serialize");
        assert_eq!(json["kind"], "complete");
    }

    // A trivial in-memory transport proves the trait is object-safe and usable —
    // the shape `InProcessTransport` (W1) and `RemoteTransport` (W3) implement.
    struct StubTransport;

    #[async_trait]
    impl BackendApi for StubTransport {
        async fn forward_mutation(
            &self,
            mutation: MutationRequest,
        ) -> Result<MutationReceipt, RuntimeError> {
            Ok(MutationReceipt {
                runtime_mutation_id: None,
                client_mutation_id: mutation.client_mutation_id,
                name: mutation.name,
                state: posthaste_runtime_contract::MutationSettlementState::Accepted,
                error: None,
                output: serde_json::Value::Null,
            })
        }

        async fn subscribe(&self, _coverage: LinkCoverage) -> Result<DownStream, RuntimeError> {
            Ok(Box::pin(futures_util::stream::iter([DownFrame::Heartbeat])))
        }
    }

    #[tokio::test]
    async fn backend_link_forwards_through_its_transport() {
        use futures_util::StreamExt;
        use posthaste_runtime_contract::ClientMutationId;

        let link = BackendLink::new(Arc::new(StubTransport));
        let receipt = link
            .forward_mutation(MutationRequest {
                session_id: None,
                name: "message.setKeywords".into(),
                args: serde_json::Value::Null,
                client_mutation_id: ClientMutationId::new("c1"),
                context: None,
            })
            .await
            .expect("forward");
        assert_eq!(receipt.client_mutation_id, ClientMutationId::new("c1"));

        let mut down = link.subscribe(LinkCoverage::Complete).await.expect("subscribe");
        assert_eq!(down.next().await, Some(DownFrame::Heartbeat));
    }
}
