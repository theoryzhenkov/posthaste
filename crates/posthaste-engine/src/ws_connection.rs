use std::sync::Arc;

use jmap_client::client::Client;
use jmap_client::client_ws::CorrelatedWs;
use jmap_client::core::request::Request;
use jmap_client::core::response::{Response, TaggedMethodResponse};
use jmap_client::PushObject;
use posthaste_observability::{events, ph_debug, ph_info, ph_warn};
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::live::map_gateway_error;
use posthaste_domain_model::GatewayError;

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
    Connected(ActiveWs),
}

/// A live WebSocket plus the machinery that keeps the shared connection healthy.
///
/// The fork's reader task multiplexes API responses and push notifications off a
/// single socket, but delivers pushes over a *bounded* channel and **blocks**
/// when it fills (`client_ws.rs`, `push_tx.send(..).await`). If nothing drains
/// pushes promptly, that block also stalls API-response correlation on the same
/// socket -- so an in-flight `send()` hangs behind a backlog of pushes (M67).
///
/// To decouple the two, `ActiveWs` owns a dedicated drain task that continuously
/// pulls pushes out of the fork's channel into an unbounded engine-side queue.
/// The fork reader therefore never blocks on push backpressure, so API responses
/// always flow; push consumers read the engine-side queue instead.
struct ActiveWs {
    correlated: Arc<CorrelatedWs>,
    /// Engine-side push queue, fed by the drain task. A `tokio::Mutex` gives the
    /// single push consumer `&mut` access to `recv()` behind the shared `RwLock`.
    push_rx: Arc<Mutex<mpsc::UnboundedReceiver<Result<PushObject, jmap_client::Error>>>>,
    /// Aborts the drain task when this state is dropped/replaced (disconnect or
    /// reconnect), so a stale drainer never lingers on an old socket.
    _drain: DrainGuard,
}

/// Aborts the wrapped push-drain task on drop.
struct DrainGuard(tokio::task::JoinHandle<()>);

impl Drop for DrainGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl ActiveWs {
    /// Wrap a freshly-connected `CorrelatedWs`, spawning the push-drain task that
    /// keeps the fork reader from wedging on push backpressure (M67).
    fn spawn(correlated: Arc<CorrelatedWs>) -> Self {
        let (push_tx, push_rx) = mpsc::unbounded_channel();
        let drain_source = correlated.clone();
        let drain = tokio::spawn(async move {
            // Pull pushes as fast as the fork emits them. The forward is
            // non-blocking (unbounded), so the fork's bounded push buffer is
            // emptied promptly and its reader never blocks -- API responses on
            // the shared socket keep correlating. If no consumer is attached the
            // push is dropped, but we keep draining so the reader stays live.
            while let Some(item) = drain_source.next_push().await {
                let _ = push_tx.send(item);
            }
        });
        Self {
            correlated,
            push_rx: Arc::new(Mutex::new(push_rx)),
            _drain: DrainGuard(drain),
        }
    }
}

impl WsConnectionState {
    fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }

    fn connection(&self) -> Option<Arc<CorrelatedWs>> {
        match self {
            Self::Disconnected => None,
            Self::Connected(active) => Some(active.correlated.clone()),
        }
    }

    fn push_queue(&self) -> Option<Arc<Mutex<mpsc::UnboundedReceiver<Result<PushObject, jmap_client::Error>>>>> {
        match self {
            Self::Disconnected => None,
            Self::Connected(active) => Some(active.push_rx.clone()),
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
        *guard = WsConnectionState::Connected(ActiveWs::spawn(Arc::new(ws)));
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

    /// Like [`send`](Self::send) but surfaces the raw `jmap_client::Error` so the
    /// send path can apply phase-based dispatch classification (the duplicate-send
    /// fix) instead of the transport-blind [`map_gateway_error`]. A "not
    /// connected" state is a pre-write condition (nothing was sent), reported as
    /// an internal error the send classifier maps to a safe retryable transient.
    pub async fn send_raw(
        &self,
        request: Request<'_>,
    ) -> Result<Response<TaggedMethodResponse>, jmap_client::Error> {
        let ws = self
            .state
            .read()
            .await
            .connection()
            .ok_or_else(|| jmap_client::Error::Internal("WebSocket not connected".to_string()))?;
        ws.send(request).await
    }

    /// Read the next push notification from the shared WS.
    ///
    /// Reads from the engine-side drain queue (fed by `ActiveWs`'s drain task),
    /// not directly from the fork -- so a slow push consumer applies backpressure
    /// only here, never to the fork reader that also correlates API responses
    /// (M67). The read `RwLock` is released before awaiting `recv`, so a
    /// concurrent `send()`/`disconnect()` is never blocked by a waiting consumer.
    ///
    /// @spec docs/L1-jmap#push
    pub async fn next_push(&self) -> Option<Result<PushObject, jmap_client::Error>> {
        let queue = self.state.read().await.push_queue()?;
        let mut rx = queue.lock().await;
        rx.recv().await
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

    /// Send a WebSocket keepalive ping over the shared connection (PP1/D88).
    ///
    /// This is the sole production caller of the fork's `CorrelatedWs::ws_ping`.
    /// A dead-enough socket makes the underlying write fail here; a NAT half-open
    /// socket (write still buffers) is instead caught by the push consumer's
    /// read-deadline. Either way the connection stops silently masquerading as
    /// alive.
    ///
    /// @spec docs/L2-transport#websocket-connection-lifecycle
    pub async fn ws_ping(&self) -> Result<(), GatewayError> {
        let ws = self
            .state
            .read()
            .await
            .connection()
            .ok_or_else(|| GatewayError::Network("WebSocket not connected".to_string()))?;
        ws.ws_ping().await.map_err(map_gateway_error)
    }

    /// Clear the WS connection state (e.g. after a connection error).
    ///
    /// @spec docs/L2-transport#http-fallback
    pub async fn disconnect(&self) {
        let mut guard = self.state.write().await;
        *guard = WsConnectionState::Disconnected;
    }
}
