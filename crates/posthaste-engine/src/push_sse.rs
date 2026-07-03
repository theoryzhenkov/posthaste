use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use jmap_client::client::Client;
use posthaste_domain_model::{AccountId, GatewayError, PushStream};
use posthaste_domain_service::PushTransport;
use posthaste_observability::{events, ph_debug, ph_warn};

use crate::live::map_gateway_error;
use crate::push_common::convert_sse_push_notification;

/// Read deadline for the SSE push stream (PP1/D88). The server is asked to send a
/// keepalive ping every 60 s (see `open` below); silence past this window means
/// the stream is dead. Set above 60 s so a healthy ping cycle never trips it.
/// **Review.**
///
/// NOTE (fork follow-up): the pinned jmap-client fork surfaces SSE keepalive
/// pings as stream items only under its `debug` feature; in release builds a
/// genuinely idle-but-healthy stream can trip this deadline and reconnect. That
/// is bounded and non-lossy — every reconnect runs an unconditional catch-up sync
/// (PP3) — but a future fork change should surface the keepalive so an idle
/// healthy stream is not needlessly recycled. SSE is the *fallback* transport
/// (WS is preferred), which bounds the blast radius.
pub(crate) const PUSH_SSE_READ_DEADLINE: Duration = Duration::from_secs(90);

/// Push transport that reads JMAP state-change notifications via Server-Sent Events.
///
/// Used as a fallback when the server does not advertise WebSocket capability.
/// Wraps `jmap_client::Client::event_source()`.
///
/// @spec docs/L2-transport#pushtransport
/// @spec docs/L1-jmap#push
pub struct SsePushTransport {
    client: Arc<Client>,
    server_account_id: String,
}

impl SsePushTransport {
    /// Create an SSE push transport wrapping an authenticated JMAP client.
    pub fn new(client: Arc<Client>, server_account_id: String) -> Self {
        Self {
            client,
            server_account_id,
        }
    }
}

#[async_trait]
impl PushTransport for SsePushTransport {
    /// Transport identifier used in logging and push status tracking.
    fn name(&self) -> &'static str {
        "sse"
    }

    /// Client-enforced read deadline for the SSE push stream (PP1/D88): the
    /// resilient wrapper declares the stream dead if no item arrives within this
    /// window, rather than trusting the server-side ping it requested but never
    /// verified.
    fn read_deadline(&self) -> Duration {
        PUSH_SSE_READ_DEADLINE
    }

    /// Open an EventSource connection and return a filtered stream of `PushNotification`.
    ///
    /// Resumes from `checkpoint` (SSE last-event-id) when provided.
    ///
    /// @spec docs/L2-transport#http-fallback
    /// @spec docs/L1-jmap#push
    async fn open(
        &self,
        account_id: &AccountId,
        checkpoint: Option<&str>,
    ) -> Result<Option<PushStream>, GatewayError> {
        let target_url = self.client.session().event_source_url().to_string();
        ph_debug!(
            events::PUSH_SSE_STREAM_OPENING,
            account_id = %account_id,
            server_account_id = %self.server_account_id,
            target_url = %target_url,
            checkpoint,
            "opening SSE push stream"
        );
        let stream = self
            .client
            .event_source(
                crate::WATCHED_DATA_TYPES
                    .into_iter()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .into(),
                false,
                Some(60),
                checkpoint,
            )
            .await
            .map_err(|error| {
                let mapped = map_gateway_error(error);
                ph_warn!(
                    events::PUSH_SSE_CONNECTION_FAILED,
                    account_id = %account_id,
                    target_url = %target_url,
                    error = %mapped,
                    "SSE connection failed"
                );
                mapped
            })?;

        let account_id = account_id.clone();
        let server_account_id = self.server_account_id.clone();
        Ok(Some(Box::pin(stream.filter_map(move |event| {
            let account_id = account_id.clone();
            let server_account_id = server_account_id.clone();
            async move {
                match event {
                    Ok(push) => {
                        convert_sse_push_notification(&account_id, &server_account_id, push)
                            .transpose()
                    }
                    Err(error) => Some(Err(map_gateway_error(error))),
                }
            }
        }))))
    }
}
