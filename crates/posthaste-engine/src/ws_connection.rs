use std::sync::Arc;

use jmap_client::client::Client;
use jmap_client::client_ws::CorrelatedWs;
use jmap_client::core::request::Request;
use jmap_client::core::response::{Response, TaggedMethodResponse};
use jmap_client::PushObject;
use posthaste_observability::{events, ph_debug, ph_info, ph_warn};
use tokio::sync::RwLock;

use crate::live::map_gateway_error;
use posthaste_domain::GatewayError;

/// A shared WebSocket connection that supports both API calls and push.
///
/// Created once per account when the server advertises WebSocket push support.
/// Dropped when the account connection tears down. The same connection
/// carries interleaved API responses and push notifications, demultiplexed
/// by the `CorrelatedWs` layer in `jmap-client`.
///
/// @spec docs/L2-transport#websocket-connection-lifecycle
/// @spec docs/L2-transport#single-ws-per-account
pub struct SharedWsConnection {
    client: Arc<Client>,
    state: RwLock<WsConnectionState>,
}

enum WsConnectionState {
    Disconnected,
    Connected(Arc<CorrelatedWs>),
}

impl WsConnectionState {
    fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }

    fn connection(&self) -> Option<Arc<CorrelatedWs>> {
        match self {
            Self::Disconnected => None,
            Self::Connected(ws) => Some(ws.clone()),
        }
    }
}

impl SharedWsConnection {
    /// Create a new shared connection holder (not yet connected).
    pub fn new(client: Arc<Client>) -> Self {
        Self {
            client,
            state: RwLock::new(WsConnectionState::Disconnected),
        }
    }

    /// Return the WebSocket URL from the JMAP session, if the server advertises it.
    pub fn ws_url(&self) -> Option<String> {
        self.client
            .session()
            .websocket_capabilities()
            .map(|caps| caps.url().to_string())
    }

    /// Open the WS connection if not already active.
    ///
    /// Uses double-checked locking: reads first, upgrades to write only if needed.
    ///
    /// @spec docs/L2-transport#websocket-connection-lifecycle
    pub async fn ensure_connected(&self) -> Result<(), GatewayError> {
        {
            let guard = self.state.read().await;
            if guard.is_connected() {
                return Ok(());
            }
        }
        let mut guard = self.state.write().await;
        // Double-check after acquiring write lock
        if guard.is_connected() {
            return Ok(());
        }
        let target_url = self.ws_url();
        ph_debug!(
            events::JMAP_WEBSOCKET_CONNECTION_OPENING,
            target_url = target_url.as_deref(),
            "opening WebSocket connection"
        );
        let ws = self.client.connect_ws_correlated().await.map_err(|error| {
            let mapped = map_gateway_error(error);
            ph_warn!(
                events::JMAP_WEBSOCKET_CONNECTION_FAILED,
                target_url = target_url.as_deref(),
                error = %mapped,
                "WebSocket connection failed"
            );
            mapped
        })?;
        *guard = WsConnectionState::Connected(Arc::new(ws));
        ph_info!(
            events::JMAP_WEBSOCKET_CONNECTION_ESTABLISHED,
            target_url = target_url.as_deref(),
            "WebSocket connection established"
        );
        Ok(())
    }

    /// Check if a WS connection is currently active.
    pub async fn is_connected(&self) -> bool {
        self.state.read().await.is_connected()
    }

    /// Send a JMAP request over WebSocket.
    ///
    /// Caller should check `is_connected()` first; if WS is disconnected,
    /// this returns a connection error. Responses are correlated by request ID.
    ///
    /// @spec docs/L2-transport#requestresponse-correlation
    pub async fn send(
        &self,
        request: Request<'_>,
    ) -> Result<Response<TaggedMethodResponse>, GatewayError> {
        let ws = self
            .state
            .read()
            .await
            .connection()
            .ok_or_else(|| GatewayError::Network("WebSocket not connected".to_string()))?;
        ws.send(request).await.map_err(map_gateway_error)
    }

    /// Read the next push notification from the shared WS.
    ///
    /// @spec docs/L1-jmap#push
    pub async fn next_push(&self) -> Option<Result<PushObject, jmap_client::Error>> {
        let ws = self.state.read().await.connection()?;
        ws.next_push().await
    }

    /// Enable push notifications on the WS connection for watched data types.
    ///
    /// @spec docs/L2-transport#websocket-connection-lifecycle
    /// @spec docs/L1-jmap#push
    pub async fn enable_push(&self, checkpoint: Option<&str>) -> Result<(), GatewayError> {
        let ws = self
            .state
            .read()
            .await
            .connection()
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
    /// @spec docs/L2-transport#http-fallback
    pub async fn disconnect(&self) {
        let mut guard = self.state.write().await;
        *guard = WsConnectionState::Disconnected;
    }
}
