use std::sync::Arc;

use jmap_client::client::Client;
use jmap_client::client_ws::CorrelatedWs;
use jmap_client::core::request::Request;
use jmap_client::core::response::{Response, TaggedMethodResponse};
use jmap_client::PushObject;
use tokio::sync::RwLock;

use crate::live::map_gateway_error;
use mail_domain::GatewayError;

/// A shared WebSocket connection that supports both API calls and push.
///
/// Created once per account when the server advertises WebSocket capability.
/// Dropped when the account connection tears down. The same connection
/// carries interleaved API responses and push notifications, demultiplexed
/// by the `CorrelatedWs` layer in `jmap-client`.
///
/// @spec spec/L2-transport#websocket-connection-lifecycle
/// @spec spec/L2-transport#single-ws-per-account
pub struct SharedWsConnection {
    client: Arc<Client>,
    ws: RwLock<Option<CorrelatedWs>>,
}

impl SharedWsConnection {
    /// Create a new shared connection holder (not yet connected).
    pub fn new(client: Arc<Client>) -> Self {
        Self {
            client,
            ws: RwLock::new(None),
        }
    }

    /// Open the WS connection if not already active.
    ///
    /// Uses double-checked locking: reads first, upgrades to write only if needed.
    ///
    /// @spec spec/L2-transport#websocket-connection-lifecycle
    pub async fn ensure_connected(&self) -> Result<(), GatewayError> {
        {
            let guard = self.ws.read().await;
            if guard.is_some() {
                return Ok(());
            }
        }
        let mut guard = self.ws.write().await;
        // Double-check after acquiring write lock
        if guard.is_some() {
            return Ok(());
        }
        let ws = self
            .client
            .connect_ws_correlated()
            .await
            .map_err(map_gateway_error)?;
        *guard = Some(ws);
        Ok(())
    }

    /// Check if a WS connection is currently active.
    pub async fn is_connected(&self) -> bool {
        self.ws.read().await.is_some()
    }

    /// Send a JMAP request over WebSocket.
    ///
    /// Caller should check `is_connected()` first; if WS is disconnected,
    /// this returns a connection error. Responses are correlated by request ID.
    ///
    /// @spec spec/L2-transport#requestresponse-correlation
    pub async fn send(
        &self,
        request: Request<'_>,
    ) -> Result<Response<TaggedMethodResponse>, GatewayError> {
        let guard = self.ws.read().await;
        let ws = guard
            .as_ref()
            .ok_or_else(|| GatewayError::Network("WebSocket not connected".to_string()))?;
        ws.send(request).await.map_err(map_gateway_error)
    }

    /// Read the next push notification from the shared WS.
    ///
    /// @spec spec/L1-jmap#push
    pub async fn next_push(&self) -> Option<Result<PushObject, jmap_client::Error>> {
        let guard = self.ws.read().await;
        let ws = guard.as_ref()?;
        // Release the RwLock before awaiting — next_push has its own internal lock.
        // We need to hold a reference to ws though, so we can't drop guard.
        // This is safe: the read lock allows concurrent readers, and CorrelatedWs
        // methods are safe for concurrent use.
        ws.next_push().await
    }

    /// Enable push notifications on the WS connection for watched data types.
    ///
    /// @spec spec/L2-transport#websocket-connection-lifecycle
    /// @spec spec/L1-jmap#push
    pub async fn enable_push(&self, checkpoint: Option<&str>) -> Result<(), GatewayError> {
        let guard = self.ws.read().await;
        let ws = guard
            .as_ref()
            .ok_or_else(|| GatewayError::Network("WebSocket not connected".to_string()))?;
        ws.enable_push_ws(
            Some(crate::WATCHED_DATA_TYPES),
            checkpoint.map(String::from),
        )
        .await
        .map_err(map_gateway_error)
    }

    /// Clear the WS connection state (e.g. after a connection error).
    ///
    /// @spec spec/L2-transport#http-fallback
    pub async fn disconnect(&self) {
        let mut guard = self.ws.write().await;
        *guard = None;
    }
}
