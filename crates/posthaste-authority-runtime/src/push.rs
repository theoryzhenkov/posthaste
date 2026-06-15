use futures_util::StreamExt;
use posthaste_domain::{
    AccountId, PushEventStream, PushStreamEvent, PushTransport, ResilientPushConfig,
};
use posthaste_observability::{events, ph_debug, ph_warn};

/// Which transport is currently being used by the resilient stream.
enum ActiveTransport {
    Primary,
    Fallback,
}

/// Build a resilient push notification stream that reconnects with backoff
/// and falls back from the primary transport (WS) to the fallback (SSE)
/// after repeated failures.
///
/// @spec docs/L2-transport#resilientpushstream
/// @spec docs/L2-transport#http-fallback
pub fn resilient_push_stream(
    account_id: AccountId,
    primary: Box<dyn PushTransport>,
    fallback: Option<Box<dyn PushTransport>>,
    config: ResilientPushConfig,
) -> PushEventStream {
    Box::pin(async_stream::stream! {
        let mut active = ActiveTransport::Primary;
        let mut consecutive_failures: u32 = 0;
        let mut current_delay = config.initial_retry_delay;
        let mut checkpoint: Option<String> = None;

        loop {
            let transport: &dyn PushTransport = match active {
                ActiveTransport::Primary => &*primary,
                ActiveTransport::Fallback => match &fallback {
                    Some(fb) => &**fb,
                    None => &*primary,
                },
            };

            match transport.open(&account_id, checkpoint.as_deref()).await {
                Ok(Some(mut stream)) => {
                    yield PushStreamEvent::Connected {
                        transport: transport.name(),
                    };
                    consecutive_failures = 0;
                    current_delay = config.initial_retry_delay;

                    let mut disconnected = false;
                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(notification) => {
                                if notification.checkpoint.is_some() {
                                    checkpoint.clone_from(&notification.checkpoint);
                                }
                                yield PushStreamEvent::Notification(notification);
                            }
                            Err(error) => {
                                yield PushStreamEvent::Disconnected {
                                    transport: transport.name(),
                                    reason: error.to_string(),
                                };
                                disconnected = true;
                                break;
                            }
                        }
                    }

                    if !disconnected {
                        yield PushStreamEvent::Disconnected {
                            transport: transport.name(),
                            reason: "stream ended".to_string(),
                        };
                    }

                    // Stream ended or errored — count as failure for fallback logic
                    consecutive_failures += 1;
                }
                Ok(None) => {
                    ph_debug!(
                        events::PUSH_TRANSPORT_UNSUPPORTED,
                        account_id = %account_id,
                        transport = transport.name(),
                        "transport unsupported by server"
                    );
                    consecutive_failures += 1;
                }
                Err(error) => {
                    ph_warn!(
                        events::PUSH_TRANSPORT_OPEN_FAILED,
                        account_id = %account_id,
                        transport = transport.name(),
                        error = %error,
                        attempt = consecutive_failures + 1,
                        fallback_threshold = config.fallback_threshold,
                        "push transport open failed"
                    );
                    yield PushStreamEvent::Disconnected {
                        transport: transport.name(),
                        reason: error.to_string(),
                    };
                    consecutive_failures += 1;
                }
            }

            // Check if we should fall back
            if consecutive_failures >= config.fallback_threshold {
                if let Some(ref fb) = fallback {
                    match active {
                        ActiveTransport::Primary => {
                            ph_warn!(
                                events::PUSH_FALLBACK_TRIGGERED,
                                account_id = %account_id,
                                from = primary.name(),
                                to = fb.name(),
                                consecutive_failures,
                                "push transport fallback triggered"
                            );
                            active = ActiveTransport::Fallback;
                            consecutive_failures = 0;
                            current_delay = config.initial_retry_delay;
                            yield PushStreamEvent::Fallback {
                                from: primary.name(),
                                to: fb.name(),
                            };
                            continue; // try fallback immediately
                        }
                        ActiveTransport::Fallback => {
                            // Fallback also exhausted, cycle back to primary
                            ph_warn!(
                                events::PUSH_FALLBACK_CYCLED_TO_PRIMARY,
                                account_id = %account_id,
                                from = fb.name(),
                                to = primary.name(),
                                consecutive_failures,
                                "push fallback exhausted, cycling back to primary"
                            );
                            active = ActiveTransport::Primary;
                            consecutive_failures = 0;
                            current_delay = config.initial_retry_delay;
                            continue;
                        }
                    }
                }
            }

            ph_debug!(
                events::PUSH_RECONNECT_BACKOFF,
                account_id = %account_id,
                delay_ms = current_delay.as_millis(),
                attempt = consecutive_failures,
                fallback_threshold = config.fallback_threshold,
                "push reconnect backoff"
            );
            tokio::time::sleep(current_delay).await;
            current_delay = (current_delay * 2).min(config.max_retry_delay);
        }
    })
}

#[cfg(test)]
mod tests;
