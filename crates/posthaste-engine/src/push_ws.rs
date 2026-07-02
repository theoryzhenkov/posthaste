use std::sync::Arc;

use async_trait::async_trait;
use posthaste_domain_service::{AccountId, GatewayError, PushStream, PushTransport};
use posthaste_observability::{events, ph_debug, ph_warn};

use crate::live::map_gateway_error;
use crate::push_common::convert_ws_push_object;
use crate::ws_connection::SharedWsConnection;

/// Push transport that reads JMAP state-change notifications from a shared WebSocket.
///
/// Preferred over SSE when the server advertises `urn:ietf:params:jmap:websocket`.
/// The underlying connection is shared with API request routing via `SharedWsConnection`.
///
/// @spec docs/L2-transport#pushtransport
/// @spec docs/L2-transport#websocket-connection-lifecycle
pub struct WsPushTransport {
    ws: Arc<SharedWsConnection>,
    server_account_id: String,
}

impl WsPushTransport {
    /// Create a WebSocket push transport wrapping an existing shared connection.
    pub fn new(ws: Arc<SharedWsConnection>, server_account_id: String) -> Self {
        Self {
            ws,
            server_account_id,
        }
    }
}

#[async_trait]
impl PushTransport for WsPushTransport {
    /// Transport identifier used in logging and push status tracking.
    fn name(&self) -> &'static str {
        "ws"
    }

    /// Ensure the WS connection is active, enable push, and return a stream
    /// of `PushNotification` values filtered from WS messages.
    ///
    /// @spec docs/L2-transport#websocket-connection-lifecycle
    /// @spec docs/L1-jmap#push
    async fn open(
        &self,
        account_id: &AccountId,
        checkpoint: Option<&str>,
    ) -> Result<Option<PushStream>, GatewayError> {
        let target_url = self.ws.ws_url();
        ph_debug!(
            events::PUSH_WS_STREAM_OPENING,
            account_id = %account_id,
            server_account_id = %self.server_account_id,
            target_url = target_url.as_deref(),
            checkpoint,
            "opening WS push stream"
        );
        self.ws.ensure_connected().await?;
        self.ws.enable_push(checkpoint).await?;

        let ws = self.ws.clone();
        let account_id = account_id.clone();
        let server_account_id = self.server_account_id.clone();

        Ok(Some(Box::pin(async_stream::stream! {
            loop {
                match ws.next_push().await {
                    Some(Ok(push)) => {
                        match convert_ws_push_object(&account_id, &server_account_id, push) {
                            Ok(Some(notification)) => yield Ok(notification),
                            Ok(None) => {}
                            Err(error) => yield Err(error),
                        }
                    }
                    Some(Err(error)) => {
                        let mapped = map_gateway_error(error);
                        ph_warn!(
                            events::PUSH_WS_STREAM_ERROR,
                            account_id = %account_id,
                            error = %mapped,
                            "WS push stream error"
                        );
                        ws.disconnect().await;
                        yield Err(mapped);
                        return;
                    }
                    None => {
                        ph_debug!(
                            events::PUSH_WS_STREAM_ENDED,
                            account_id = %account_id,
                            "WS push stream ended"
                        );
                        ws.disconnect().await;
                        return;
                    }
                }
            }
        })))
    }
}
