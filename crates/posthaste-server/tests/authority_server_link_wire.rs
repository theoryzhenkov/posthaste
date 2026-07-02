//! The runtime↔authority-server link over the real remote wire: the production
//! `link_router` (far-node HTTP surface) served against the production
//! `RemoteAuthorityServer` (near-node client), over loopback HTTP.
//!
//! Proves the two ends meet on the shared contract — the up-channel POST returns
//! the authority server's receipt, the down-channel SSE delivers base-assertion frames —
//! without a mock standing in for either side. The far node here is a stub
//! transport (no store), so the test stays fast and self-contained; W1/W2 cover
//! the in-process transport applying real mutations.
//!
//! @spec docs/replication/authority-server-link/L2#2-backendapi-implementations-localbackend-remotebackend

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures_util::StreamExt;
use posthaste_authority_server_link::{
    AuthorityServerApi, AuthorityServerFrame, AuthorityServerLink, AuthorityServerLinkHandle,
    AuthorityServerLinkId, BaseAssertion, BaseUpdate, DownStream, LinkCoverage,
};
use posthaste_link_core::MessageFoldState;
use posthaste_contract_core::{
    ClientMutationId, MutationReceipt, MutationRequest, MutationSettlementState, RuntimeError,
    RuntimeMutationId,
};
use posthaste_runtime::RemoteAuthorityServer;
use posthaste_authority_server::{link_router, LinkAuth};

/// A far node that records the forwarded mutation and serves one base assertion.
/// The Api half is all defaults (this stub carries no read channel); every real
/// transport implements the pair (D33).
struct StubFarNode;

#[async_trait]
impl AuthorityServerApi for StubFarNode {}

#[async_trait]
impl AuthorityServerLink for StubFarNode {
    async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        Ok(MutationReceipt {
            runtime_mutation_id: Some(RuntimeMutationId::new("authority-server-1")),
            client_mutation_id: mutation.client_mutation_id,
            name: mutation.operation.name().to_string(),
            state: MutationSettlementState::Confirmed,
            error: None,
            output: serde_json::json!({ "events": [] }),
        })
    }

    async fn subscribe(&self, _coverage: LinkCoverage) -> Result<DownStream, RuntimeError> {
        let frame = AuthorityServerFrame::Base {
            assertions: vec![BaseAssertion {
                account_id: "acct".into(),
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
    let router = link_router(
        AuthorityServerLinkHandle::new(Arc::new(StubFarNode)),
        LinkAuth::Disabled,
    );
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
    let transport = RemoteAuthorityServer::new(base_url);

    let receipt = transport
        .forward_mutation(MutationRequest {
            session_id: None,
            operation: serde_json::from_value(serde_json::json!({
                "name": "message.setFlaggedState",
                "args": serde_json::json!({ "sourceId": "acct", "messageId": "m1", "flagged": true }),
            }))
            .expect("typed operation parses"),
            client_mutation_id: ClientMutationId::new("c1"),
            context: None,
        })
        .await
        .expect("forward over the wire");

    assert_eq!(receipt.client_mutation_id, ClientMutationId::new("c1"));
    assert_eq!(
        receipt.runtime_mutation_id,
        Some(RuntimeMutationId::new("authority-server-1"))
    );
    assert_eq!(receipt.state, MutationSettlementState::Confirmed);
}

#[tokio::test]
async fn remote_transport_reads_the_link_router_down_channel() {
    let base_url = serve_far_node().await;
    let transport = RemoteAuthorityServer::new(base_url);

    let mut down = transport
        .subscribe(LinkCoverage::Complete)
        .await
        .expect("subscribe over the wire");
    let frame = down.next().await.expect("a base-assertion frame");

    assert_eq!(
        frame,
        AuthorityServerFrame::Base {
            assertions: vec![BaseAssertion {
                account_id: "acct".into(),
                message_id: "m1".into(),
                update: BaseUpdate::Present(MessageFoldState {
                    keywords: vec!["$flagged".into()],
                    mailbox_ids: vec!["inbox".into()],
                }),
            }],
        }
    );
}

// Startup mounts the link surface as `.nest("/v1", api).merge(link_router(..))`.
// The link path consts are the full `/v1/link/*`, while the nest registers
// concrete `/v1/...` routes, so the two coexist. Route insertion panics on
// conflict — reaching the end of this test means there is none, guarding the
// live mount (W6b) against a startup panic.
#[test]
fn link_router_merges_under_a_v1_nest_without_route_conflict() {
    let api: axum::Router =
        axum::Router::new().route("/sources", axum::routing::get(|| async { "ok" }));
    let _app: axum::Router = axum::Router::new().nest("/v1", api).merge(link_router(
        AuthorityServerLinkHandle::new(Arc::new(StubFarNode)),
        LinkAuth::Disabled,
    ));
}

// A far node that captures the `AuthorityServerLinkId` the link router threaded into
// `forward_mutation_for` — proves the auth-derived identity reaches the
// up-channel (S2): a token presented by the near node resolves to a `AuthorityServerLinkId`
// on the serve side and is carried into the runtime-aware up-channel call.
struct CapturingFarNode {
    seen_runtime_id: Mutex<Option<String>>,
}

#[async_trait]
impl AuthorityServerApi for CapturingFarNode {}

#[async_trait]
impl AuthorityServerLink for CapturingFarNode {
    async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        // The link router routes the up-channel through `forward_mutation_for`;
        // this runtime-naive fallback is only for direct (non-link) callers.
        self.forward_mutation_for(&AuthorityServerLinkId::new("<direct>"), mutation)
            .await
    }

    async fn forward_mutation_for(
        &self,
        runtime_id: &AuthorityServerLinkId,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        *self.seen_runtime_id.lock().unwrap() = Some(runtime_id.as_str().to_string());
        Ok(MutationReceipt {
            runtime_mutation_id: Some(RuntimeMutationId::new("authority-server-1")),
            client_mutation_id: mutation.client_mutation_id,
            name: mutation.operation.name().to_string(),
            state: MutationSettlementState::Confirmed,
            error: None,
            output: serde_json::json!({ "events": [] }),
        })
    }

    async fn subscribe(&self, _coverage: LinkCoverage) -> Result<DownStream, RuntimeError> {
        Ok(Box::pin(futures_util::stream::iter(Vec::new())))
    }
}

#[tokio::test]
async fn link_router_threads_the_authed_runtime_id_into_forward_mutation_for() {
    let node = Arc::new(CapturingFarNode {
        seen_runtime_id: Mutex::new(None),
    });
    // X = 1: one runtime, "rt-1", authenticated by token "t1".
    let router = link_router(
        AuthorityServerLinkHandle::new(node.clone()),
        LinkAuth::PerRuntime(HashMap::from([(
            "t1".to_string(),
            AuthorityServerLinkId::new("rt-1"),
        )])),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let base_url = format!("http://{addr}");

    // The near node presents "t1"; the authority server resolves it to AuthorityServerLinkId "rt-1".
    let transport = RemoteAuthorityServer::with_token(base_url, Some("t1".to_string()));
    transport
        .forward_mutation(MutationRequest {
            session_id: None,
            operation: serde_json::from_value(serde_json::json!({
                "name": "message.setFlaggedState",
                "args": serde_json::json!({ "sourceId": "acct", "messageId": "m1", "flagged": true }),
            }))
            .expect("typed operation parses"),
            client_mutation_id: ClientMutationId::new("c1"),
            context: None,
        })
        .await
        .expect("the mutation is forwarded");

    assert_eq!(
        node.seen_runtime_id.lock().unwrap().as_deref(),
        Some("rt-1"),
        "the link router must thread the auth-derived AuthorityServerLinkId into forward_mutation_for"
    );
}
