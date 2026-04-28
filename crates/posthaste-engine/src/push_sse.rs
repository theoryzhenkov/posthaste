use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use jmap_client::client::Client;
use posthaste_domain::{AccountId, GatewayError, PushStream, PushTransport};

use tracing::{debug, warn};

use crate::live::map_gateway_error;
use crate::push_common::convert_sse_push_notification;

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
        debug!(
            account_id = %account_id,
            server_account_id = %self.server_account_id,
            target_url = %target_url,
            checkpoint,
            "opening SSE push stream"
        );
        let stream = self
            .client
            .event_source(
                crate::WATCHED_DATA_TYPES.into_iter().collect::<Vec<_>>().into_iter().into(),
                false,
                Some(60),
                checkpoint,
            )
            .await
            .map_err(|error| {
                let mapped = map_gateway_error(error);
                warn!(account_id = %account_id, target_url = %target_url, error = %mapped, "SSE connection failed");
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
