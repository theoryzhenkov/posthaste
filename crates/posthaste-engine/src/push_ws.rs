use std::sync::Arc;

use async_trait::async_trait;
use posthaste_domain::{
    now_iso8601 as domain_now_iso8601, AccountId, GatewayError, PushNotification, PushStream,
    PushTransport,
};

use tracing::{debug, warn};

use crate::live::map_gateway_error;
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
}

impl WsPushTransport {
    /// Create a WebSocket push transport wrapping an existing shared connection.
    pub fn new(ws: Arc<SharedWsConnection>) -> Self {
        Self { ws }
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
        debug!(account_id = %account_id, target_url = target_url.as_deref(), checkpoint, "opening WS push stream");
        self.ws.ensure_connected().await?;
        self.ws.enable_push(checkpoint).await?;

        let ws = self.ws.clone();
        let account_id = account_id.clone();

        Ok(Some(Box::pin(async_stream::stream! {
            loop {
                match ws.next_push().await {
                    Some(Ok(push)) => {
                        match convert_push_object(&account_id, push) {
                            Ok(Some(notification)) => yield Ok(notification),
                            Ok(None) => continue,
                            Err(error) => yield Err(error),
                        }
                    }
                    Some(Err(error)) => {
                        let mapped = map_gateway_error(error);
                        warn!(account_id = %account_id, error = %mapped, "WS push stream error");
                        yield Err(mapped);
                        return;
                    }
                    None => {
                        debug!(account_id = %account_id, "WS push stream ended");
                        return;
                    }
                }
            }
        })))
    }
}

/// Convert a raw JMAP `PushObject` into a domain `PushNotification`.
///
/// Only `StateChange` variants are relevant; other push object types are ignored.
/// WS push does not carry SSE-style checkpoint IDs, so the notification's
/// `checkpoint` is always `None`.
///
/// @spec docs/L1-jmap#push
fn convert_push_object(
    account_id: &AccountId,
    push: jmap_client::PushObject,
) -> Result<Option<PushNotification>, GatewayError> {
    match push {
        jmap_client::PushObject::StateChange { mut changed } => {
            let changed_types = changed
                .remove(account_id.as_str())
                .map(|entries| entries.into_keys().map(|dt| dt.to_string()).collect())
                .unwrap_or_default();
            let received_at = domain_now_iso8601().map_err(GatewayError::Rejected)?;
            Ok(Some(PushNotification {
                account_id: account_id.clone(),
                changed: changed_types,
                received_at,
                // WS push notifications don't carry SSE-style event IDs.
                // The ResilientPushStream will pass the last SSE checkpoint
                // (if any) on reconnect, but WS connections cannot resume
                // from a checkpoint. A full delta sync after WS reconnect
                // handles this correctly.
                checkpoint: None,
            }))
        }
        _ => Ok(None),
    }
}
