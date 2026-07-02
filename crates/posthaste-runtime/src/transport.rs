//! [`RemoteAuthorityServer`]: the remote [`AuthorityServerLink`] implementation the near node uses
//! when the authority server lives in another process or host.
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

use posthaste_domain_service::{
    AccountId, ConversationId, ConversationView, MessageDetail, MessageId, MessageSummary,
};
use posthaste_authority_server_link::{
    AuthorityServerLink, AuthorityServerFrame, DownStream, LinkCoverage, LINK_CONVERSATION_PATH, LINK_DETAIL_PATH,
    LINK_FORWARD_MUTATION_PATH, LINK_QUERY_PATH, LINK_SUBSCRIBE_PATH, LINK_SUMMARY_PATH,
};
use posthaste_contract_core::{
    MailQueryPage, MailQueryRequest, MutationReceipt, MutationRequest, RuntimeError,
    RuntimeErrorCode,
};

// The default transport: the runtime calls the co-located authority server directly.

/// The remote link transport ([replication authority-server-link L2 §2](../replication/authority-server-link/L2.md)): a near
/// node talking to a far node that serves the link wire over HTTP. The
/// up-channel `POST`s named mutations; the down-channel is an SSE stream of
/// base-assertion frames. This is what lets the authority server live on another
/// process or host; it is selected by config, the symmetric twin of the
/// in-process transport.
pub struct RemoteAuthorityServer {
    base_url: String,
    client: reqwest::Client,
    /// The link bearer token presented on every request, when the authority server's
    /// `link_router` requires one ([`LinkAuth::PerRuntime`](posthaste_server)). `None`
    /// for an unauthenticated link (in-process tests / dormant mounts).
    token: Option<String>,
}

impl RemoteAuthorityServer {
    pub fn new(base_url: String) -> Self {
        Self::with_token(base_url, None)
    }

    /// A remote transport that presents `token` (when `Some`) as a bearer
    /// credential on every link request.
    pub fn with_token(base_url: String, token: Option<String>) -> Self {
        Self {
            // Trim a trailing slash so `base_url + path` never doubles it.
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            token,
        }
    }

    /// Attach the link bearer token to a request, if configured.
    fn authed(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.token {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        }
    }

    /// POST a JSON request to a link path and parse the JSON response — the one
    /// HTTP round-trip the generated [`AuthorityServerLink`] methods (and the bespoke
    /// request/response ones) share
    /// ([`for_each_link_op`](posthaste_authority_server_link::for_each_link_op)). Carries
    /// the link bearer token.
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

/// Parse one SSE event block (the text between `\n\n` boundaries) into a
/// [`AuthorityServerFrame`]. SSE carries the JSON frame on one or more `data:` lines;
/// non-data lines (comments, `event:`/`id:`) are ignored. Returns `None` for a
/// keep-alive comment or an unparseable block. Pure, so it is unit-testable
/// without a live stream.
pub(crate) fn parse_sse_frame(block: &str) -> Option<AuthorityServerFrame> {
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

/// Emit the full [`RemoteAuthorityServer`] [`AuthorityServerLink`] impl: the bespoke up-channel
/// (`forward_mutation`) + SSE down-channel (`subscribe`) + the pre-existing read
/// methods, plus one generated method per link-op row. Emitting the whole
/// `#[async_trait] impl` from the macro is deliberate: `async_trait` then runs
/// on the already-expanded impl, so it desugars the generated methods too (a
/// `macro_rules!` invocation *inside* an `#[async_trait]` impl would expand too
/// late and the generated methods would miss the desugaring).
macro_rules! remote_authority_server_impl {
    ($($method:ident => $path:literal => $req:ident { $($field:ident : $fty:ty),* $(,)? } => $ret:ty;)*) => {
#[async_trait]
impl AuthorityServerLink for RemoteAuthorityServer {
    async fn forward_mutation(
        &self,
        mutation: MutationRequest,
    ) -> Result<MutationReceipt, RuntimeError> {
        self.post_link(LINK_FORWARD_MUTATION_PATH, &mutation).await
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

    async fn subscribe(&self, coverage: LinkCoverage) -> Result<DownStream, RuntimeError> {
        let url = format!("{}{}", self.base_url, LINK_SUBSCRIBE_PATH);
        let coverage_param = serde_json::to_string(&coverage).map_err(|error| {
            RuntimeError::internal(format!("failed to encode coverage: {error}"), None)
        })?;
        let response = self
            .authed(self.client.get(&url))
            .query(&[("coverage", coverage_param)])
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

    // The full request/response surface (reads + typed writes) — generated from
    // the shared link-op table so it cannot drift from the server handlers.
    $(
        async fn $method(&self, $($field: $fty),*) -> Result<$ret, RuntimeError> {
            self.post_link($path, &posthaste_authority_server_link::$req { $($field),* }).await
        }
    )*
}
    };
}
posthaste_authority_server_link::for_each_link_op!(remote_authority_server_impl);

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_authority_server_link::{BaseAssertion, BaseUpdate};
    use posthaste_link_core::MessageFoldState;
    use posthaste_contract_core::{MutationSettlementState, RuntimeMutationId};
    use serde_json::json;

    fn fold(keywords: &[&str], mailboxes: &[&str]) -> MessageFoldState {
        MessageFoldState {
            keywords: keywords.iter().map(|k| k.to_string()).collect(),
            mailbox_ids: mailboxes.iter().map(|m| m.to_string()).collect(),
        }
    }

    #[test]
    fn parse_sse_frame_reads_a_data_line_as_a_down_frame() {
        let frame = AuthorityServerFrame::Base {
            assertions: vec![BaseAssertion {
                account_id: "acct".into(),
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
            let frame = AuthorityServerFrame::Base {
                assertions: vec![BaseAssertion {
                    account_id: "acct".into(),
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
            .subscribe(LinkCoverage::Complete)
            .await
            .expect("subscribe");
        let frame = down.next().await.expect("a down frame");
        assert_eq!(
            frame,
            AuthorityServerFrame::Base {
                assertions: vec![BaseAssertion {
                    account_id: "acct".into(),
                    message_id: "m1".into(),
                    update: BaseUpdate::Present(fold(&["$flagged"], &["inbox"])),
                }],
            }
        );
    }
}
