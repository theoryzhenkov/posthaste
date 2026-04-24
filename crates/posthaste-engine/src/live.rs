use std::sync::Arc;

use async_trait::async_trait;
use jmap_client::client::Client;
use jmap_client::core::error::MethodErrorType;
use jmap_client::mailbox;
use posthaste_domain::{
    AccountId, BlobId, FetchedBody, GatewayError, Identity, MailGateway, MailboxId, MessageId,
    MutationOutcome, PushTransport, ReplyContext, SendMessageRequest, SetKeywordsCommand,
    SyncBatch, SyncCursor,
};

use tracing::{debug, info, instrument};

/// Discover and connect to a JMAP server, returning a configured client.
///
/// Performs session discovery via `.well-known/jmap`, authenticates with
/// basic credentials, and follows redirects scoped to the server's host.
///
/// @spec docs/L1-jmap#session
/// @spec docs/L1-jmap#authentication
#[instrument(skip(password))]
pub async fn connect_jmap_client(
    url: &str,
    username: &str,
    password: &str,
) -> Result<Arc<Client>, GatewayError> {
    debug!("connecting to JMAP server");
    let host = url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(String::from))
        .unwrap_or_default();
    let client = Client::new()
        .credentials((username, password))
        .follow_redirects([host])
        .connect(url)
        .await
        .map_err(map_gateway_error)?;
    let session = client.session();
    let ws_url = session
        .websocket_capabilities()
        .map(|caps| caps.url().to_string());
    let ws_push = session
        .websocket_capabilities()
        .map(|caps| caps.supports_push())
        .unwrap_or(false);
    info!(
        api_url = session.api_url(),
        event_source_url = session.event_source_url(),
        ws_url = ws_url.as_deref(),
        ws_push,
        "JMAP session established"
    );
    Ok(Arc::new(client))
}

/// Production `MailGateway` backed by a live JMAP server connection.
///
/// Holds an authenticated `jmap_client::Client` and, when the server
/// advertises WebSocket capability, a `SharedWsConnection` used for
/// interactive API calls and push notifications.
///
/// @spec docs/L1-jmap#session
/// @spec docs/L2-transport#transport-negotiation
pub struct LiveJmapGateway {
    client: Arc<Client>,
    ws: Option<Arc<crate::ws_connection::SharedWsConnection>>,
}

impl LiveJmapGateway {
    /// Wrap an already-connected client, opening a WebSocket if the server supports it.
    ///
    /// @spec docs/L2-transport#transport-negotiation
    pub fn from_client(client: Arc<Client>) -> Self {
        let ws = if client.session().websocket_capabilities().is_some() {
            debug!("WebSocket capability available, creating shared connection");
            Some(Arc::new(crate::ws_connection::SharedWsConnection::new(
                client.clone(),
            )))
        } else {
            debug!("WebSocket capability not advertised, WS transport disabled");
            None
        };
        Self { client, ws }
    }

    /// Discover, authenticate, and construct a gateway in one step.
    ///
    /// @spec docs/L1-jmap#session
    pub async fn connect(url: &str, username: &str, password: &str) -> Result<Self, GatewayError> {
        let client = connect_jmap_client(url, username, password).await?;
        Ok(Self::from_client(client))
    }

    /// Borrow the underlying JMAP client for direct access.
    pub fn client(&self) -> &Arc<Client> {
        &self.client
    }

    /// Route a JMAP request through WebSocket if connected, HTTP otherwise.
    ///
    /// Currently only interactive methods (mutations, body fetch, identity,
    /// compose) use this. Sync helpers still use HTTP convenience methods
    /// directly. TODO: once jmap-client supports transparent WS routing in
    /// Client::send(), all paths will use WS automatically and this method
    /// can be removed.
    ///
    /// @spec docs/L2-transport#jmaptransport
    /// @spec docs/L2-transport#http-fallback
    pub(crate) async fn send_request(
        &self,
        request: jmap_client::core::request::Request<'_>,
    ) -> Result<
        jmap_client::core::response::Response<jmap_client::core::response::TaggedMethodResponse>,
        GatewayError,
    > {
        if let Some(ref ws) = self.ws {
            if ws.is_connected().await {
                return ws.send(request).await;
            }
        }
        request.send().await.map_err(map_gateway_error)
    }

    pub(crate) async fn fetch_mailbox_id_by_role(
        &self,
        role: mailbox::Role,
    ) -> Result<MailboxId, GatewayError> {
        let mut request = self.client.build();
        request.get_mailbox().properties([
            mailbox::Property::Id,
            mailbox::Property::Name,
            mailbox::Property::Role,
        ]);
        let mut response = self.send_request(request).await?;
        let mailboxes = required_method_response(response.pop_method_response(), "Mailbox/get")?
            .unwrap_get_mailbox()
            .map_err(map_gateway_error)?
            .take_list();

        mailboxes
            .into_iter()
            .find(|mailbox| mailbox.role() == role)
            .and_then(|mailbox| mailbox.id().map(|id| MailboxId::from(id.to_string())))
            .ok_or_else(|| {
                GatewayError::Rejected(format!("required {:?} mailbox not available", role))
            })
    }

    pub(crate) fn ws(&self) -> Option<&Arc<crate::ws_connection::SharedWsConnection>> {
        self.ws.as_ref()
    }
}

pub(crate) fn required_method_response<T>(
    response: Option<T>,
    method: &str,
) -> Result<T, GatewayError> {
    response.ok_or_else(|| GatewayError::Rejected(format!("{method} response missing")))
}

/// @spec docs/L1-jmap#method-calls
/// @spec docs/L1-sync#sync-loop
/// @spec docs/L2-transport#gateway-unchanged
#[async_trait]
impl MailGateway for LiveJmapGateway {
    /// Perform a full sync cycle: mailbox state then email state.
    ///
    /// Falls back from delta to full sync on `cannotCalculateChanges`.
    ///
    /// @spec docs/L1-sync#sync-loop
    /// @spec docs/L1-sync#state-management
    async fn sync(
        &self,
        _account_id: &AccountId,
        cursors: &[SyncCursor],
    ) -> Result<SyncBatch, GatewayError> {
        crate::live_sync::sync_account(&self.client, cursors).await
    }

    /// Lazily fetch the body content of a single message via `Email/get`.
    ///
    /// Bodies are not synced during metadata sync; they are fetched on first
    /// view and cached locally.
    ///
    /// @spec docs/L1-sync#sync-granularity
    /// @spec docs/L1-jmap#methods-used
    async fn fetch_message_body(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError> {
        crate::live_message::fetch_message_body(self, message_id).await
    }

    async fn download_blob(
        &self,
        _account_id: &AccountId,
        blob_id: &BlobId,
    ) -> Result<Vec<u8>, GatewayError> {
        crate::live_message::download_blob(self, blob_id).await
    }

    /// Add or remove keywords (flags) on a message via `Email/set`.
    ///
    /// Uses `ifInState` for optimistic concurrency when `expected_state` is provided.
    ///
    /// @spec docs/L1-jmap#methods-used
    /// @spec docs/L1-sync#conflict-model
    async fn set_keywords(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError> {
        crate::live_mutation::set_keywords(self, message_id, expected_state, command).await
    }

    /// Replace a message's mailbox membership via `Email/set`.
    ///
    /// Used for move and archive operations. Supports optimistic concurrency.
    ///
    /// @spec docs/L1-jmap#methods-used
    /// @spec docs/L1-sync#conflict-model
    async fn replace_mailboxes(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError> {
        crate::live_mutation::replace_mailboxes(self, message_id, expected_state, mailbox_ids).await
    }

    /// Permanently destroy a message via `Email/set`.
    ///
    /// @spec docs/L1-jmap#methods-used
    /// @spec docs/L1-sync#conflict-model
    async fn destroy_message(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError> {
        crate::live_mutation::destroy_message(self, message_id, expected_state).await
    }

    /// Fetch the primary sender identity for an account via `Identity/get`.
    ///
    /// @spec docs/L1-jmap#methods-used
    /// @spec docs/L1-compose#composesession-interface
    async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
        crate::live_compose::fetch_identity(self).await
    }

    /// Fetch the original message metadata needed for reply/forward composition.
    ///
    /// Retrieves subject, sender, recipients, threading headers, and quoted
    /// body text. The body is `>` prefixed for reply quoting.
    ///
    /// @spec docs/L1-compose#reply-quoting
    /// @spec docs/L1-compose#forward-quoting
    async fn fetch_reply_context(
        &self,
        _account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError> {
        crate::live_compose::fetch_reply_context(self, message_id).await
    }

    /// Send a message via `Email/set` + `EmailSubmission/set` in a single JMAP request.
    ///
    /// Renders the Markdown body to HTML and constructs a multipart/alternative
    /// MIME structure. The server handles Sent folder placement.
    ///
    /// @spec docs/L1-compose#mime-structure
    /// @spec docs/L1-jmap#methods-used
    async fn send_message(
        &self,
        account_id: &AccountId,
        request_data: &SendMessageRequest,
    ) -> Result<(), GatewayError> {
        crate::live_compose::send_message(self, account_id, request_data).await
    }

    /// Return available push transports, preferring WebSocket over SSE.
    ///
    /// @spec docs/L2-transport#pushtransport
    fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
        crate::live_push::push_transports(self)
    }
}

/// Map `jmap_client::Error` into the typed `GatewayError` enum.
///
/// Distinguishes auth errors (401), state mismatches, `cannotCalculateChanges`,
/// and generic network/method errors.
///
/// @spec docs/L1-jmap#error-model
pub(crate) fn map_gateway_error(error: jmap_client::Error) -> GatewayError {
    match error {
        jmap_client::Error::Problem(problem) => {
            if problem.status == Some(401) {
                GatewayError::Auth
            } else {
                GatewayError::Network(problem.to_string())
            }
        }
        jmap_client::Error::Method(method) => match method.p_type {
            MethodErrorType::StateMismatch => GatewayError::StateMismatch,
            MethodErrorType::CannotCalculateChanges => GatewayError::CannotCalculateChanges,
            _ => GatewayError::Rejected(method.to_string()),
        },
        jmap_client::Error::Set(error) => GatewayError::Rejected(error.to_string()),
        other => GatewayError::Network(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_method_response_becomes_gateway_rejected_error() {
        let error = required_method_response::<()>(None, "Email/get")
            .expect_err("missing responses should be rejected");

        match error {
            GatewayError::Rejected(message) => {
                assert_eq!(message, "Email/get response missing");
            }
            other => panic!("expected rejected error, got {other:?}"),
        }
    }

    #[test]
    fn set_errors_become_gateway_rejected_errors() {
        let set_error: jmap_client::core::set::SetError<String> =
            serde_json::from_value(serde_json::json!({
                "type": "noRecipients",
                "description": "No recipients found in email."
            }))
            .expect("set error should deserialize");

        let error = map_gateway_error(set_error.into());

        match error {
            GatewayError::Rejected(message) => {
                assert_eq!(message, "noRecipients: No recipients found in email.");
            }
            other => panic!("expected rejected error, got {other:?}"),
        }
    }
}
