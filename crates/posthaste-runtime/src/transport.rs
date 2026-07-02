//! [`RemoteAuthorityServer`]: the remote implementation of the far-node trait
//! pair ([`AuthorityServerApi`] + [`AuthorityServerLink`], D33) the near node
//! uses when the authority server lives in another process or host.
//!
//! The split case ([replication authority-server-link L2 §2](../replication/authority-server-link/L2.md)):
//! the up-channel `POST`s named mutations, the reads `POST` request/response, and
//! the down-channel is an SSE stream of base-assertion frames. The in-process
//! counterpart (`LocalAuthorityServer`, direct calls to a co-located far node) lives in
//! the far-node crate; both are config-selected
//! ([replication authority-server-link L2 §6](../replication/authority-server-link/L2.md)).
//!
//! @spec docs/replication/authority-server-link/L2#2-backendapi-implementations-localbackend-remotebackend

use async_trait::async_trait;
use futures_util::StreamExt;

use posthaste_domain_model::{
    AccountId, CommandAck, ConversationId, ConversationView, MessageDetail, MessageId,
    MessageSummary,
};
use posthaste_authority_server_link::{
    AuthorityServerApi, AuthorityServerLink, DownStream, LinkCoverage, MailCommandRequest,
    SequencedFrame, LINK_CONVERSATION_PATH, LINK_DETAIL_PATH, LINK_QUERY_PATH,
    LINK_SUBSCRIBE_PATH, LINK_SUMMARY_PATH,
};
use posthaste_contract_core::{
    MailOperation, MailQueryPage, MailQueryRequest, MutationReceipt, MutationRequest,
    RuntimeError, RuntimeErrorCode,
};
use tokio::sync::mpsc;

use crate::link_near_end::{sse_payloads, NativeNearEnd};

// The default transport: the runtime calls the co-located authority server directly.

/// The remote link transport ([replication authority-server-link L2 §2](../replication/authority-server-link/L2.md)): a near
/// node talking to a far node that serves the link wire over HTTP. The
/// up-channel `POST`s named mutations; the down-channel is an SSE stream of
/// base-assertion frames. This is what lets the authority server live on another
/// process or host; it is selected by config, the symmetric twin of the
/// in-process transport.
///
/// The link's resilience-bearing halves — `forward_mutation` and the production
/// down-channel — are driven by the shared `LinkNearEnd` engine
/// ([`crate::link_near_end`], D40): request deadline, jittered capped backoff,
/// permanent-vs-transient classification, and the reconnect loop that owns the
/// `afterSeq` resume cursor all come from the engine's config — no policy lives
/// here. The request/response reads (`post_link`) and the raw one-shot
/// [`Self::subscribe`] (the wire primitive tests exercise; the engine opens its
/// own streams) remain plain reqwest.
pub struct RemoteAuthorityServer {
    base_url: String,
    client: reqwest::Client,
    /// The link bearer token presented on every request, when the authority server's
    /// `link_router` requires one ([`LinkAuth::PerRuntime`](posthaste_server)). `None`
    /// for an unauthenticated link (in-process tests / dormant mounts).
    token: Option<String>,
    /// The near-end engine actor (dedicated thread): up-channel forwards + the
    /// resilient down-channel.
    near_end: NativeNearEnd,
}

impl RemoteAuthorityServer {
    pub fn new(base_url: String) -> Self {
        Self::with_token(base_url, None)
    }

    /// A remote transport that presents `token` (when `Some`) as a bearer
    /// credential on every link request.
    pub fn with_token(base_url: String, token: Option<String>) -> Self {
        // Trim a trailing slash so `base_url + path` never doubles it.
        let base_url = base_url.trim_end_matches('/').to_string();
        let client = reqwest::Client::new();
        let near_end = NativeNearEnd::spawn(client.clone(), base_url.clone(), token.clone());
        Self {
            base_url,
            client,
            token,
            near_end,
        }
    }

    /// Take the engine-driven down-channel: starts the engine's reconnect loop
    /// (subscribe → consume → resubscribe from the engine-owned `afterSeq`
    /// cursor, jittered backoff between attempts) and hands back the frame
    /// receiver the read path consumes. `None` after the first take.
    pub fn take_down_channel(&self) -> Option<mpsc::UnboundedReceiver<SequencedFrame>> {
        self.near_end.take_down_channel()
    }

    /// Attach the link bearer token to a request, if configured.
    fn authed(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        }
    }

    /// POST a JSON request to a link path and parse the JSON response — the one
    /// HTTP round-trip the generated [`AuthorityServerApi`]/[`AuthorityServerLink`]
    /// methods (and the bespoke request/response ones) share
    /// ([`for_each_link_api_op`](posthaste_authority_server_link::for_each_link_api_op)).
    /// Carries the link bearer token.
    async fn post_link<Req, Ret>(&self, path: &str, req: &Req) -> Result<Ret, RuntimeError>
    where
        Req: serde::Serialize,
        Ret: serde::de::DeserializeOwned,
    {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .authed(self.client.post(&url))
            .json(req)
            .send()
            .await
            .map_err(transport_error)?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RuntimeError::new(
                RuntimeErrorCode::GatewayRejected,
                format!("remote authority server rejected link request ({status}): {body}"),
            ));
        }
        response.json::<Ret>().await.map_err(transport_error)
    }
}

/// Map a transport-layer (network) failure to a retryable disconnected error —
/// the link is down, not the request invalid.
fn transport_error(error: reqwest::Error) -> RuntimeError {
    RuntimeError::retryable(
        RuntimeErrorCode::TransportDisconnected,
        format!("runtime↔authority-server link transport error: {error}"),
    )
}

/// Emit the full [`RemoteAuthorityServer`] [`AuthorityServerApi`] impl: the
/// bespoke read methods (`query_mail_page`/`current_summary`/`message_detail`/
/// `conversation`) + the direct-apply command entry (`apply`, dispatched onto
/// the five preserved per-command routes via [`MailCommandRequest`]), plus one
/// generated method per Api-op row. Emitting the whole `#[async_trait] impl`
/// from the macro is deliberate: `async_trait` then runs on the
/// already-expanded impl, so it desugars the generated methods too (a
/// `macro_rules!` invocation *inside* an `#[async_trait]` impl would expand too
/// late and the generated methods would miss the desugaring).
macro_rules! remote_authority_server_api_impl {
    ($($method:ident => $path:literal => $req:ident { $($field:ident : $fty:ty),* $(,)? } => $ret:ty;)*) => {
#[async_trait]
impl AuthorityServerApi for RemoteAuthorityServer {
    /// Direct-apply a mail operation: project it onto its preserved per-command
    /// wire route (byte-identical to the pre-split per-command RPC) and POST.
    /// Replica-only operations are rejected locally, before any round trip.
    async fn apply(&self, op: MailOperation) -> Result<CommandAck, RuntimeError> {
        let command = MailCommandRequest::from_operation(op)?;
        self.post_link(command.path(), &command).await
    }

    async fn query_mail_page(
        &self,
        request: MailQueryRequest,
    ) -> Result<MailQueryPage, RuntimeError> {
        self.post_link(LINK_QUERY_PATH, &request).await
    }

    async fn current_summary(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageSummary>, RuntimeError> {
        self.post_link(
            LINK_SUMMARY_PATH,
            &serde_json::json!({ "accountId": account_id, "messageId": message_id }),
        )
        .await
    }

    async fn message_detail(
        &self,
        account_id: AccountId,
        message_id: MessageId,
    ) -> Result<Option<MessageDetail>, RuntimeError> {
        self.post_link(
            LINK_DETAIL_PATH,
            &serde_json::json!({ "accountId": account_id, "messageId": message_id }),
        )
        .await
    }

    async fn conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<ConversationView, RuntimeError> {
        self.post_link(
            LINK_CONVERSATION_PATH,
            &serde_json::json!({ "conversationId": conversation_id }),
        )
        .await
    }

    // The full request/response surface (reads + typed writes) — generated from
    // the shared Api-op table so it cannot drift from the server handlers.
    $(
        async fn $method(&self, $($field: $fty),*) -> Result<$ret, RuntimeError> {
            self.post_link($path, &posthaste_authority_server_link::$req { $($field),* }).await
        }
    )*
}
    };
}
posthaste_authority_server_link::for_each_link_api_op!(remote_authority_server_api_impl);

/// Emit the full [`RemoteAuthorityServer`] [`AuthorityServerLink`] impl: the
/// bespoke up-channel (`forward_mutation`) + SSE down-channel (`subscribe`),
/// plus one generated method per op-lifecycle row (same whole-impl emission
/// rationale as [`remote_authority_server_api_impl`]).
macro_rules! remote_authority_server_link_impl {
    ($($method:ident => $path:literal => $req:ident { $($field:ident : $fty:ty),* $(,)? } => $ret:ty;)*) => {
#[async_trait]
impl AuthorityServerLink for RemoteAuthorityServer {
    /// Forward through the shared near-end engine: request deadline, jittered
    /// backoff retry of transient failures, permanent 4xx surfaced without a
    /// retry — the engine config is the only policy source (D40; fixes
    /// lifecycle-debt row 1's deadline-less POST).
    async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        self.near_end.forward(mutation).await
    }

    /// The raw wire primitive: ONE subscription attempt, no resilience. The
    /// production down-channel does not call this — it rides the engine's
    /// reconnect loop ([`Self::take_down_channel`]); this remains the trait's
    /// one-shot stream for in-process parity and the wire tests.
    async fn subscribe(
        &self,
        coverage: LinkCoverage,
        after_seq: Option<u64>,
    ) -> Result<DownStream, RuntimeError> {
        let url = format!("{}{}", self.base_url, LINK_SUBSCRIBE_PATH);
        let coverage_param = serde_json::to_string(&coverage).map_err(|error| {
            RuntimeError::internal(format!("failed to encode coverage: {error}"), None)
        })?;
        // Coverage says WHAT to stream; `after_seq` says WHERE to resume (D46).
        let mut query: Vec<(&str, String)> = vec![("coverage", coverage_param)];
        if let Some(after) = after_seq {
            query.push(("afterSeq", after.to_string()));
        }
        let response = self
            .authed(self.client.get(&url))
            .query(&query)
            .send()
            .await
            .map_err(transport_error)?;
        if !response.status().is_success() {
            let status = response.status();
            return Err(RuntimeError::retryable(
                RuntimeErrorCode::TransportDisconnected,
                format!("remote authority server refused link subscription ({status})"),
            ));
        }
        // One SSE framing fact (`sse_payloads`, shared with the engine's native
        // transport); the JSON parse into the typed envelope happens here at
        // the boundary — an unparseable payload is dropped, never cast.
        let stream = sse_payloads(response.bytes_stream())
            .filter_map(|payload| async move {
                serde_json::from_str::<SequencedFrame>(&payload).ok()
            });
        Ok(Box::pin(stream))
    }

    // The op-lifecycle mutations — generated from the shared lifecycle-op table
    // so they cannot drift from the server handlers.
    $(
        async fn $method(&self, $($field: $fty),*) -> Result<$ret, RuntimeError> {
            self.post_link($path, &posthaste_authority_server_link::$req { $($field),* }).await
        }
    )*
}
    };
}
posthaste_authority_server_link::for_each_link_lifecycle_op!(remote_authority_server_link_impl);

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_authority_server_link::{
        AuthorityServerFrame, BaseAssertion, BaseUpdate, LINK_FORWARD_MUTATION_PATH,
    };
    use posthaste_link_core::MessageFoldState;
    use posthaste_contract_core::{MutationSettlementState, RuntimeMutationId};
    use serde_json::json;

    fn fold(keywords: &[&str], mailboxes: &[&str]) -> MessageFoldState {
        MessageFoldState {
            keywords: keywords.iter().map(|k| k.to_string()).collect(),
            mailbox_ids: mailboxes.iter().map(|m| m.to_string()).collect(),
        }
    }

    // A mock far-node HTTP surface stands in for the authority server's (W3b) link
    // endpoints, proving the RemoteAuthorityServer client speaks the wire end to end:
    // POST up returns a receipt, SSE down yields a base-assertion frame.
    #[tokio::test]
    async fn remote_transport_round_trips_against_a_mock_far_node() {
        use axum::response::sse::{Event, Sse};
        use axum::routing::{get, post};
        use axum::{Json, Router};
        use posthaste_contract_core::ClientMutationId;
        use std::convert::Infallible;

        async fn forward(Json(request): Json<MutationRequest>) -> Json<MutationReceipt> {
            Json(MutationReceipt {
                runtime_mutation_id: Some(RuntimeMutationId::new("authority-server-1")),
                client_mutation_id: request.client_mutation_id,
                name: request.operation.name().to_string(),
                state: MutationSettlementState::Confirmed,
                error: None,
                output: serde_json::Value::Null,
            })
        }

        async fn subscribe(
        ) -> Sse<futures_util::stream::Iter<std::vec::IntoIter<Result<Event, Infallible>>>>
        {
            let sequenced = SequencedFrame::new(
                1,
                AuthorityServerFrame::Base {
                    assertions: vec![BaseAssertion {
                        account_id: "acct".into(),
                        message_id: "m1".into(),
                        update: BaseUpdate::Present(fold(&["$flagged"], &["inbox"])),
                    }],
                },
            );
            let event = Event::default().data(serde_json::to_string(&sequenced).unwrap());
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

        let transport = RemoteAuthorityServer::new(format!("http://{addr}"));

        let receipt = transport
            .forward_mutation(
                serde_json::from_value(json!({
                    "name": "message.setFlaggedState",
                    "args": { "sourceId": "acct", "messageId": "m1", "flagged": true },
                    "clientMutationId": "c1",
                }))
                .expect("request builds from the flat wire shape"),
            )
            .await
            .expect("forward");
        assert_eq!(receipt.client_mutation_id, ClientMutationId::new("c1"));
        assert_eq!(
            receipt.runtime_mutation_id,
            Some(RuntimeMutationId::new("authority-server-1"))
        );

        let mut down = transport
            .subscribe(LinkCoverage::Complete, None)
            .await
            .expect("subscribe");
        let sequenced = down.next().await.expect("a down frame");
        assert_eq!(sequenced.seq, 1);
        assert_eq!(
            sequenced.frame,
            AuthorityServerFrame::Base {
                assertions: vec![BaseAssertion {
                    account_id: "acct".into(),
                    message_id: "m1".into(),
                    update: BaseUpdate::Present(fold(&["$flagged"], &["inbox"])),
                }],
            }
        );
    }

    // The engine-driven down-channel (M9b2 native adoption): each subscription
    // serves ONE frame then closes; the engine must reconnect on its own and
    // resume from the engine-owned cursor (`afterSeq`) — the subscribe-once-
    // and-die loop is gone (lifecycle-debt row 2), and resume consumes the last
    // seen seq (D46).
    #[tokio::test]
    async fn down_channel_engine_reconnects_and_resumes_from_the_cursor() {
        use axum::extract::{Query, State};
        use axum::response::sse::{Event, Sse};
        use axum::routing::get;
        use axum::Router;
        use std::collections::HashMap;
        use std::convert::Infallible;
        use std::sync::{Arc, Mutex};

        #[derive(Default)]
        struct SubscribeLog {
            after_seqs: Mutex<Vec<Option<u64>>>,
            connections: std::sync::atomic::AtomicU64,
        }

        async fn subscribe(
            State(log): State<Arc<SubscribeLog>>,
            Query(query): Query<HashMap<String, String>>,
        ) -> Sse<futures_util::stream::Iter<std::vec::IntoIter<Result<Event, Infallible>>>>
        {
            let after = query.get("afterSeq").and_then(|s| s.parse().ok());
            log.after_seqs.lock().unwrap().push(after);
            let seq = log
                .connections
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                + 1;
            let sequenced = SequencedFrame::new(
                seq,
                AuthorityServerFrame::Base {
                    assertions: vec![BaseAssertion {
                        account_id: "acct".into(),
                        message_id: format!("m{seq}"),
                        update: BaseUpdate::Removed,
                    }],
                },
            );
            let event = Event::default().data(serde_json::to_string(&sequenced).unwrap());
            // One frame, then the stream closes — forcing a reconnect.
            Sse::new(futures_util::stream::iter(vec![Ok(event)]))
        }

        let log = Arc::new(SubscribeLog::default());
        let app = Router::new()
            .route(LINK_SUBSCRIBE_PATH, get(subscribe))
            .with_state(log.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let transport = RemoteAuthorityServer::new(format!("http://{addr}"));
        let mut frames = transport
            .take_down_channel()
            .expect("first take yields the down channel");
        assert!(
            transport.take_down_channel().is_none(),
            "one consumer owns the channel"
        );

        // Two frames can only arrive over two subscriptions (one frame each).
        let first = frames.recv().await.expect("first frame");
        assert_eq!(first.seq, 1);
        let second = frames.recv().await.expect("second frame after reconnect");
        assert_eq!(second.seq, 2);

        let after_seqs = log.after_seqs.lock().unwrap().clone();
        assert_eq!(after_seqs[0], None, "fresh subscribe has no cursor");
        assert_eq!(
            after_seqs[1],
            Some(1),
            "the reconnect resumed from the engine-owned cursor"
        );
    }
}
