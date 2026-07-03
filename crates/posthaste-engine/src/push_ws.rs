use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use posthaste_domain_model::{AccountId, GatewayError, PushStream};
use posthaste_domain_service::PushTransport;
use posthaste_observability::{events, ph_debug, ph_warn};

use crate::live::map_gateway_error;
use crate::push_common::convert_ws_push_object;
use crate::ws_connection::SharedWsConnection;

/// Interval between client-driven WS keepalive pings (PP1/D88). Each tick drives
/// the fork's `ws_ping`; a dead-enough socket fails the write here, and a NAT
/// half-open socket is caught by the read deadline below. Set well under the read
/// deadline so a healthy link is probed several times before the deadline could
/// trip. **Review**.
pub(crate) const PUSH_WS_PING_INTERVAL: Duration = Duration::from_secs(30);
/// Read deadline for the WS push stream (PP1/D88): no traffic — a notification,
/// or a keepalive-ping *write failure* that ends the stream — within this window
/// means the connection is dead. Set above the ping interval so a healthy link's
/// ping cycle never trips it. **Review**.
pub(crate) const PUSH_WS_READ_DEADLINE: Duration = Duration::from_secs(90);

/// Aborts the wrapped task when dropped, so the keepalive pinger dies with the
/// push stream it belongs to (no orphaned pinger after a reconnect/fallback).
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

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

    /// Client-enforced read deadline for the WS push stream (PP1/D88).
    fn read_deadline(&self) -> Duration {
        PUSH_WS_READ_DEADLINE
    }

    /// Tear down the shared WS connection backing a dead stream so interactive
    /// mutations stop routing to the corpse (PP1) and re-route over HTTP.
    async fn on_dead(&self) {
        self.ws.disconnect().await;
    }

    /// Ensure the WS connection is active, enable push, start the keepalive
    /// pinger, and return a stream of `PushNotification` values filtered from WS
    /// messages.
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

        // Keepalive pinger (PP1/D88): the sole production driver of the fork's
        // `ws_ping`. Tied to the stream's lifetime via `AbortOnDrop` — a
        // reconnect/fallback that drops the stream kills the pinger.
        let ping_ws = ws.clone();
        let ping_account = account_id.clone();
        let pinger = AbortOnDrop(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(PUSH_WS_PING_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            ticker.tick().await; // consume the immediate first tick
            loop {
                ticker.tick().await;
                if let Err(error) = ping_ws.ws_ping().await {
                    ph_warn!(
                        events::PUSH_WS_KEEPALIVE_FAILED,
                        account_id = %ping_account,
                        error = %error,
                        "WS keepalive ping failed; tearing down connection"
                    );
                    // A failed write means the socket is dead: disconnect so
                    // `next_push` returns `None` and the stream ends promptly.
                    ping_ws.disconnect().await;
                    return;
                }
            }
        }));

        Ok(Some(Box::pin(async_stream::stream! {
            let _pinger = pinger; // dropped (aborted) when the stream is dropped
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
