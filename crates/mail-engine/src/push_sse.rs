use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use jmap_client::client::Client;
use mail_domain::{
    now_iso8601 as domain_now_iso8601, AccountId, GatewayError, PushNotification, PushStream,
    PushTransport,
};

use tracing::{debug, warn};

use crate::live::map_gateway_error;

/// Push transport that reads JMAP state-change notifications via Server-Sent Events.
///
/// Used as a fallback when the server does not advertise WebSocket capability.
/// Wraps `jmap_client::Client::event_source()`.
///
/// @spec docs/L2-transport#pushtransport
/// @spec docs/L1-jmap#push
pub struct SsePushTransport {
    client: Arc<Client>,
}

impl SsePushTransport {
    /// Create an SSE push transport wrapping an authenticated JMAP client.
    pub fn new(client: Arc<Client>) -> Self {
        Self { client }
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
        debug!(account_id = %account_id, target_url = %target_url, checkpoint, "opening SSE push stream");
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
        Ok(Some(Box::pin(stream.filter_map(move |event| {
            let account_id = account_id.clone();
            async move {
                match event {
                    Ok(jmap_client::event_source::PushNotification::StateChange(changes)) => {
                        let changed = changes
                            .changes(account_id.as_str())
                            .map(|entries| {
                                entries
                                    .map(|(data_type, _)| data_type.to_string())
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let received_at = match domain_now_iso8601() {
                            Ok(value) => value,
                            Err(error) => return Some(Err(GatewayError::Rejected(error))),
                        };
                        Some(Ok(PushNotification {
                            account_id,
                            changed,
                            received_at,
                            checkpoint: changes.id().map(str::to_string),
                        }))
                    }
                    Ok(jmap_client::event_source::PushNotification::CalendarAlert(_)) => None,
                    Err(error) => Some(Err(map_gateway_error(error))),
                }
            }
        }))))
    }
}
