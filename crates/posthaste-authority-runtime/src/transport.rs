//! In-process link transport over the co-located backend far node.
//!
//! [`InProcessTransport`] is the default [`LinkTransport`]
//! ([replication L4 §4](../replication/L4.md)): direct calls to a co-located
//! [`Backend`], zero serialization, instant confirmation. It is the
//! behavior-preserving seam — the runtime↔backend link carried in one process,
//! byte-for-byte the pre-link behavior (assertion `colocated-unchanged`). The
//! remote transport (POST up + SSE down) is the W3 twin selected by the same
//! config knob.
//!
//! @spec docs/replication/L4#4-the-transport-abstraction-one-seam-for-both-links

use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use posthaste_domain::{DomainEvent, EVENT_TOPIC_MESSAGE_UPDATED};
use posthaste_link_contract::{
    BaseAssertion, BaseUpdate, DownFrame, DownStream, LinkCoverage, LinkTransport,
    LINK_FORWARD_MUTATION_PATH, LINK_SUBSCRIBE_PATH,
};
use posthaste_link_core::MessageFoldState;
use posthaste_runtime_contract::{
    MutationReceipt, MutationRequest, MutationSettlementState, RuntimeError, RuntimeErrorCode,
    RuntimeMutationId,
};
use tokio::sync::broadcast;

use crate::backend::Backend;

/// The default transport: the runtime calls the co-located backend directly.
pub(crate) struct InProcessTransport {
    backend: Arc<Backend>,
}

impl InProcessTransport {
    pub(crate) fn new(backend: Arc<Backend>) -> Self {
        Self { backend }
    }
}

/// How a message domain event names its message's authoritative base change —
/// the pure half of the down-channel mapping, factored out so it is testable
/// without a running store. `current` is the message's complete fold state read
/// from the backend (`None` when the message is gone); a `deleted` event maps to
/// a removal regardless. Non-message events and events without a message id
/// produce no assertion.
pub(crate) fn message_event_to_assertion(
    event: &DomainEvent,
    current: Option<MessageFoldState>,
) -> Option<BaseAssertion> {
    if event.topic != EVENT_TOPIC_MESSAGE_UPDATED {
        return None;
    }
    let message_id = event.message_id.as_ref()?.as_str().to_string();
    let deleted = event
        .payload
        .get("deleted")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if deleted {
        return Some(BaseAssertion {
            message_id,
            update: BaseUpdate::Removed,
        });
    }
    // A present message asserts its complete current state. If the read found
    // nothing (a race with a concurrent removal), treat it as removed.
    Some(BaseAssertion {
        message_id,
        update: match current {
            Some(state) => BaseUpdate::Present(state),
            None => BaseUpdate::Removed,
        },
    })
}

#[async_trait]
impl LinkTransport for InProcessTransport {
    /// Up-channel: apply the named mutation to the co-located backend and return
    /// a receipt carrying the backend's `RuntimeMutationId` (the confirmation
    /// join key) and the command's events as `output`. In-process this is a
    /// direct call — no serialization, the mutation is applied (and confirmed)
    /// before the receipt returns.
    async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        let ack = self.backend.apply_named_message_mutation(&mutation).await?;
        let output = serde_json::to_value(&ack).map_err(|error| {
            RuntimeError::internal(
                format!("failed to serialize mutation output: {error}"),
                None,
            )
        })?;
        Ok(MutationReceipt {
            runtime_mutation_id: Some(RuntimeMutationId::new(uuid::Uuid::new_v4().to_string())),
            client_mutation_id: mutation.client_mutation_id,
            name: mutation.name,
            state: MutationSettlementState::Accepted,
            error: None,
            output,
        })
    }

    /// Down-channel: the ordered stream of authoritative base assertions. Each
    /// `message.updated` event becomes a complete [`BaseAssertion`] over its
    /// message (the far node reads the message's current summary to author the
    /// whole-message state); a `deleted` event becomes a removal. Non-message
    /// events are filtered out.
    ///
    /// In-process the up-channel confirms synchronously (the receipt returns
    /// after the effect is applied), so confirmation is carried by
    /// `forward_mutation`'s receipt rather than as a separate `Settlement`
    /// frame — those matter when the channels are decoupled (the remote
    /// transport, W3). The near node still rebases its base cache on these
    /// assertions; a co-located runtime that derives views from the cache is the
    /// W3-paired step (in-process the cache equals the store, so the view read
    /// path is unchanged today, keeping `colocated-unchanged`).
    async fn subscribe(&self, _coverage: LinkCoverage) -> Result<DownStream, RuntimeError> {
        let backend = self.backend.clone();
        let mut receiver = backend.subscribe_events();
        let stream = async_stream::stream! {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        let current = event
                            .message_id
                            .as_ref()
                            .and_then(|message_id| {
                                backend.current_fold_state(&event.account_id, message_id).ok().flatten()
                            });
                        if let Some(assertion) = message_event_to_assertion(&event, current) {
                            yield DownFrame::Base {
                                assertions: vec![assertion],
                            };
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        };
        Ok(Box::pin(stream))
    }
}

/// The remote link transport ([replication L4 §4](../replication/L4.md)): a near
/// node talking to a far node that serves the link wire over HTTP. The
/// up-channel `POST`s named mutations; the down-channel is an SSE stream of
/// base-assertion frames. This is what lets the backend live on another
/// process or host; it is selected by config, the symmetric twin of the
/// in-process transport.
pub struct RemoteTransport {
    base_url: String,
    client: reqwest::Client,
}

impl RemoteTransport {
    pub fn new(base_url: String) -> Self {
        Self {
            // Trim a trailing slash so `base_url + path` never doubles it.
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }
}

/// Map a transport-layer (network) failure to a retryable disconnected error —
/// the link is down, not the request invalid.
fn transport_error(error: reqwest::Error) -> RuntimeError {
    RuntimeError::retryable(
        RuntimeErrorCode::TransportDisconnected,
        format!("runtime↔backend link transport error: {error}"),
    )
}

/// Parse one SSE event block (the text between `\n\n` boundaries) into a
/// [`DownFrame`]. SSE carries the JSON frame on one or more `data:` lines;
/// non-data lines (comments, `event:`/`id:`) are ignored. Returns `None` for a
/// keep-alive comment or an unparseable block. Pure, so it is unit-testable
/// without a live stream.
pub(crate) fn parse_sse_frame(block: &str) -> Option<DownFrame> {
    let mut data = String::new();
    for line in block.lines() {
        if let Some(value) = line.strip_prefix("data:") {
            data.push_str(value.trim_start_matches(' '));
        }
    }
    if data.is_empty() {
        return None;
    }
    serde_json::from_str(&data).ok()
}

#[async_trait]
impl LinkTransport for RemoteTransport {
    async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        let url = format!("{}{}", self.base_url, LINK_FORWARD_MUTATION_PATH);
        let response = self
            .client
            .post(&url)
            .json(&mutation)
            .send()
            .await
            .map_err(transport_error)?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RuntimeError::new(
                RuntimeErrorCode::GatewayRejected,
                format!("remote backend rejected mutation ({status}): {body}"),
            ));
        }
        response.json::<MutationReceipt>().await.map_err(transport_error)
    }

    async fn subscribe(&self, coverage: LinkCoverage) -> Result<DownStream, RuntimeError> {
        let url = format!("{}{}", self.base_url, LINK_SUBSCRIBE_PATH);
        let coverage_param = serde_json::to_string(&coverage).map_err(|error| {
            RuntimeError::internal(format!("failed to encode coverage: {error}"), None)
        })?;
        let response = self
            .client
            .get(&url)
            .query(&[("coverage", coverage_param)])
            .send()
            .await
            .map_err(transport_error)?;
        if !response.status().is_success() {
            let status = response.status();
            return Err(RuntimeError::retryable(
                RuntimeErrorCode::TransportDisconnected,
                format!("remote backend refused link subscription ({status})"),
            ));
        }
        let mut bytes = response.bytes_stream();
        let stream = async_stream::stream! {
            // Accumulate the byte stream and emit a frame per `\n\n`-delimited
            // SSE event block.
            let mut buffer = String::new();
            while let Some(chunk) = bytes.next().await {
                let Ok(chunk) = chunk else { break };
                buffer.push_str(&String::from_utf8_lossy(&chunk));
                while let Some(boundary) = buffer.find("\n\n") {
                    let block: String = buffer.drain(..boundary + 2).collect();
                    if let Some(frame) = parse_sse_frame(&block) {
                        yield frame;
                    }
                }
            }
        };
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_domain::{AccountId, MessageId};
    use serde_json::json;

    fn message_event(payload: serde_json::Value) -> DomainEvent {
        DomainEvent {
            seq: 1,
            account_id: AccountId("acct".into()),
            topic: EVENT_TOPIC_MESSAGE_UPDATED.to_string(),
            occurred_at: "2026-06-24T00:00:00Z".into(),
            mailbox_id: None,
            message_id: Some(MessageId("m1".into())),
            payload,
        }
    }

    fn fold(keywords: &[&str], mailboxes: &[&str]) -> MessageFoldState {
        MessageFoldState {
            keywords: keywords.iter().map(|k| k.to_string()).collect(),
            mailbox_ids: mailboxes.iter().map(|m| m.to_string()).collect(),
        }
    }

    #[test]
    fn present_event_asserts_the_complete_current_state() {
        let event = message_event(json!({ "messageId": "m1", "changes": { "keywords": true } }));
        let assertion =
            message_event_to_assertion(&event, Some(fold(&["$flagged"], &["inbox"]))).unwrap();
        assert_eq!(assertion.message_id, "m1");
        assert_eq!(
            assertion.update,
            BaseUpdate::Present(fold(&["$flagged"], &["inbox"]))
        );
    }

    #[test]
    fn deleted_event_asserts_removal_regardless_of_read() {
        let event = message_event(json!({ "messageId": "m1", "deleted": true }));
        let assertion = message_event_to_assertion(&event, Some(fold(&[], &["inbox"]))).unwrap();
        assert_eq!(assertion.update, BaseUpdate::Removed);
    }

    #[test]
    fn present_event_with_missing_read_falls_back_to_removal() {
        let event = message_event(json!({ "messageId": "m1" }));
        let assertion = message_event_to_assertion(&event, None).unwrap();
        assert_eq!(assertion.update, BaseUpdate::Removed);
    }

    #[test]
    fn non_message_events_produce_no_assertion() {
        let mut event = message_event(json!({}));
        event.topic = "sync.completed".into();
        assert!(message_event_to_assertion(&event, Some(fold(&[], &[]))).is_none());
    }

    #[test]
    fn parse_sse_frame_reads_a_data_line_as_a_down_frame() {
        let frame = DownFrame::Base {
            assertions: vec![BaseAssertion {
                message_id: "m1".into(),
                update: BaseUpdate::Removed,
            }],
        };
        let data = serde_json::to_string(&frame).unwrap();
        let parsed = parse_sse_frame(&format!("data: {data}\n")).expect("frame");
        assert_eq!(parsed, frame);
    }

    #[test]
    fn parse_sse_frame_ignores_keep_alive_comments() {
        assert!(parse_sse_frame(": keep-alive\n").is_none());
        assert!(parse_sse_frame("").is_none());
    }

    // A mock far-node HTTP surface stands in for the backend's (W3b) link
    // endpoints, proving the RemoteTransport client speaks the wire end to end:
    // POST up returns a receipt, SSE down yields a base-assertion frame.
    #[tokio::test]
    async fn remote_transport_round_trips_against_a_mock_far_node() {
        use axum::response::sse::{Event, Sse};
        use axum::routing::{get, post};
        use axum::{Json, Router};
        use posthaste_runtime_contract::ClientMutationId;
        use std::convert::Infallible;

        async fn forward(Json(request): Json<MutationRequest>) -> Json<MutationReceipt> {
            Json(MutationReceipt {
                runtime_mutation_id: Some(RuntimeMutationId::new("backend-1")),
                client_mutation_id: request.client_mutation_id,
                name: request.name,
                state: MutationSettlementState::Confirmed,
                error: None,
                output: serde_json::Value::Null,
            })
        }

        async fn subscribe() -> Sse<futures_util::stream::Iter<std::vec::IntoIter<Result<Event, Infallible>>>>
        {
            let frame = DownFrame::Base {
                assertions: vec![BaseAssertion {
                    message_id: "m1".into(),
                    update: BaseUpdate::Present(fold(&["$flagged"], &["inbox"])),
                }],
            };
            let event = Event::default().data(serde_json::to_string(&frame).unwrap());
            Sse::new(futures_util::stream::iter(vec![Ok(event)]))
        }

        let app = Router::new()
            .route(LINK_FORWARD_MUTATION_PATH, post(forward))
            .route(LINK_SUBSCRIBE_PATH, get(subscribe));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let transport = RemoteTransport::new(format!("http://{addr}"));

        let receipt = transport
            .forward_mutation(MutationRequest {
                session_id: None,
                name: "message.setFlaggedState".into(),
                args: json!({ "sourceId": "acct", "messageId": "m1", "flagged": true }),
                client_mutation_id: ClientMutationId::new("c1"),
                context: None,
            })
            .await
            .expect("forward");
        assert_eq!(receipt.client_mutation_id, ClientMutationId::new("c1"));
        assert_eq!(
            receipt.runtime_mutation_id,
            Some(RuntimeMutationId::new("backend-1"))
        );

        let mut down = transport
            .subscribe(LinkCoverage::Complete)
            .await
            .expect("subscribe");
        let frame = down.next().await.expect("a down frame");
        assert_eq!(
            frame,
            DownFrame::Base {
                assertions: vec![BaseAssertion {
                    message_id: "m1".into(),
                    update: BaseUpdate::Present(fold(&["$flagged"], &["inbox"])),
                }],
            }
        );
    }
}
