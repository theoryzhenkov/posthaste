//! The runtime↔backend link over the real remote wire: the production
//! `link_router` (far-node HTTP surface) served against the production
//! `RemoteTransport` (near-node client), over loopback HTTP.
//!
//! Proves the two ends meet on the shared contract — the up-channel POST returns
//! the backend's receipt, the down-channel SSE delivers base-assertion frames —
//! without a mock standing in for either side. The far node here is a stub
//! transport (no store), so the test stays fast and self-contained; W1/W2 cover
//! the in-process transport applying real mutations.
//!
//! @spec docs/replication/L4#4-the-transport-abstraction-one-seam-for-both-links

use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use posthaste_authority_runtime::RemoteTransport;
use posthaste_link_contract::{
    BaseAssertion, BaseUpdate, DownFrame, DownStream, LinkCoverage, BackendApi,
};
use posthaste_link_core::MessageFoldState;
use posthaste_runtime_contract::{
    ClientMutationId, MutationReceipt, MutationRequest, MutationSettlementState, RuntimeError,
    RuntimeMutationId,
};
use posthaste_server::link_router;

/// A far node that records the forwarded mutation and serves one base assertion.
struct StubFarNode;

#[async_trait]
impl BackendApi for StubFarNode {
    async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        Ok(MutationReceipt {
            runtime_mutation_id: Some(RuntimeMutationId::new("backend-1")),
            client_mutation_id: mutation.client_mutation_id,
            name: mutation.name,
            state: MutationSettlementState::Confirmed,
            error: None,
            output: serde_json::json!({ "events": [] }),
        })
    }

    async fn subscribe(&self, _coverage: LinkCoverage) -> Result<DownStream, RuntimeError> {
        let frame = DownFrame::Base {
            assertions: vec![BaseAssertion {
                message_id: "m1".into(),
                update: BaseUpdate::Present(MessageFoldState {
                    keywords: vec!["$flagged".into()],
                    mailbox_ids: vec!["inbox".into()],
                }),
            }],
        };
        Ok(Box::pin(futures_util::stream::iter([frame])))
    }
}

async fn serve_far_node() -> String {
    let router = link_router(Arc::new(StubFarNode));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn remote_transport_drives_the_link_router_up_channel() {
    let base_url = serve_far_node().await;
    let transport = RemoteTransport::new(base_url);

    let receipt = transport
        .forward_mutation(MutationRequest {
            session_id: None,
            name: "message.setFlaggedState".into(),
            args: serde_json::json!({ "sourceId": "acct", "messageId": "m1", "flagged": true }),
            client_mutation_id: ClientMutationId::new("c1"),
            context: None,
        })
        .await
        .expect("forward over the wire");

    assert_eq!(receipt.client_mutation_id, ClientMutationId::new("c1"));
    assert_eq!(
        receipt.runtime_mutation_id,
        Some(RuntimeMutationId::new("backend-1"))
    );
    assert_eq!(receipt.state, MutationSettlementState::Confirmed);
}

#[tokio::test]
async fn remote_transport_reads_the_link_router_down_channel() {
    let base_url = serve_far_node().await;
    let transport = RemoteTransport::new(base_url);

    let mut down = transport
        .subscribe(LinkCoverage::Complete)
        .await
        .expect("subscribe over the wire");
    let frame = down.next().await.expect("a base-assertion frame");

    assert_eq!(
        frame,
        DownFrame::Base {
            assertions: vec![BaseAssertion {
                message_id: "m1".into(),
                update: BaseUpdate::Present(MessageFoldState {
                    keywords: vec!["$flagged".into()],
                    mailbox_ids: vec!["inbox".into()],
                }),
            }],
        }
    );
}
