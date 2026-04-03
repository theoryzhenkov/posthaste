use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::Stream;

use crate::{AccountId, GatewayError, PushNotification, PushStream};

/// A raw push transport that opens a single connection and returns a stream.
/// Stateless, does not reconnect. Implementations: SSE, WebSocket.
///
/// @spec spec/L2-transport#push-transport
#[async_trait]
pub trait PushTransport: Send + Sync {
    /// Human-readable name for logging (e.g. "ws", "sse").
    fn name(&self) -> &'static str;

    /// Open a push stream. Returns `None` if the server does not support
    /// this transport (e.g. no WebSocket capability advertised).
    async fn open(
        &self,
        account_id: &AccountId,
        checkpoint: Option<&str>,
    ) -> Result<Option<PushStream>, GatewayError>;
}

/// Events emitted by a resilient push stream alongside push notifications.
///
/// @spec spec/L2-transport#resilient-push-stream
#[derive(Clone, Debug)]
pub enum PushStreamEvent {
    /// A JMAP state-change notification.
    Notification(PushNotification),
    /// Transport connected successfully.
    Connected {
        transport: &'static str,
    },
    /// Transport disconnected or errored.
    Disconnected {
        transport: &'static str,
        reason: String,
    },
    /// Automatic transport fallback (e.g. WS to SSE).
    Fallback {
        from: &'static str,
        to: &'static str,
    },
}

/// Configuration for resilient push stream backoff and fallback behavior.
///
/// @spec spec/L2-transport#resilient-push-stream
pub struct ResilientPushConfig {
    pub initial_retry_delay: Duration,
    pub max_retry_delay: Duration,
    /// Consecutive failures on the primary transport before falling back.
    pub fallback_threshold: u32,
}

impl Default for ResilientPushConfig {
    fn default() -> Self {
        Self {
            initial_retry_delay: Duration::from_secs(5),
            max_retry_delay: Duration::from_secs(120),
            fallback_threshold: 3,
        }
    }
}

/// Async stream of [`PushStreamEvent`]s consumed by the supervisor.
///
/// @spec spec/L2-transport#resilient-push-stream
pub type PushEventStream = Pin<Box<dyn Stream<Item = PushStreamEvent> + Send>>;
