use std::sync::Arc;
use std::time::Duration;

use async_stream::stream;
use posthaste_domain_service::{
    now_iso8601, AccountId, PushEventStream, PushNotification, PushStreamEvent, SecretResolver,
};
use posthaste_observability::{events, ph_debug, ph_warn};

use crate::discovery::connect_authenticated_client;
use crate::mailbox::examine_selected_mailbox;
use crate::{ImapAdapterError, ImapConnectionConfig};

const IMAP_IDLE_RECONNECT_DELAY: Duration = Duration::from_secs(30);

/// Open an IMAP IDLE watcher as a best-effort push hint stream.
///
/// RFC 2177 IDLE is mailbox-selected and advisory: it wakes the sync loop when
/// the server reports activity, but periodic poll remains the correctness
/// fallback for missed events and unobserved mailboxes.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
/// @spec docs/L1-sync#sync-loop
pub fn imap_idle_event_stream(
    account_id: AccountId,
    config: ImapConnectionConfig,
    mailbox_name: String,
    secret_resolver: Arc<dyn SecretResolver>,
) -> PushEventStream {
    Box::pin(stream! {
        loop {
            match connect_idle_client(&config, &mailbox_name, secret_resolver.as_ref()).await {
                Ok(mut client) => {
                    yield PushStreamEvent::Connected {
                        transport: "imap-idle",
                    };

                    loop {
                        let tag = client.enqueue_idle();
                        match client.idle(tag).await {
                            Ok(()) => {
                                ph_debug!(
                                    events::IMAP_IDLE_RETURNED,
                                    account_id = %account_id,
                                    mailbox_name,
                                    "IMAP IDLE returned"
                                );
                                let received_at = match now_iso8601() {
                                    Ok(received_at) => received_at,
                                    Err(error) => {
                                        yield PushStreamEvent::Disconnected {
                                            transport: "imap-idle",
                                            reason: error,
                                        };
                                        break;
                                    }
                                };
                                yield PushStreamEvent::Notification(imap_idle_notification(
                                    account_id.clone(),
                                    received_at,
                                ));
                            }
                            Err(error) => {
                                ph_warn!(
                                    events::IMAP_IDLE_DISCONNECTED,
                                    account_id = %account_id,
                                    mailbox_name,
                                    error = ?error,
                                    "IMAP IDLE disconnected"
                                );
                                yield PushStreamEvent::Disconnected {
                                    transport: "imap-idle",
                                    reason: format!("{error:?}"),
                                };
                                break;
                            }
                        }
                    }
                }
                Err(error) => {
                    ph_warn!(
                        events::IMAP_IDLE_CONNECT_FAILED,
                        account_id = %account_id,
                        mailbox_name,
                        error = %error,
                        "IMAP IDLE connect failed"
                    );
                    yield PushStreamEvent::Disconnected {
                        transport: "imap-idle",
                        reason: error.to_string(),
                    };
                }
            }

            tokio::time::sleep(IMAP_IDLE_RECONNECT_DELAY).await;
        }
    })
}

async fn connect_idle_client(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    secret_resolver: &dyn SecretResolver,
) -> Result<imap_client::client::tokio::Client, ImapAdapterError> {
    let secret = secret_resolver
        .resolve_secret()
        .await
        .map_err(|error| ImapAdapterError::Auth(error.to_string()))?;
    let mut resolved_config = config.clone();
    resolved_config.secret = secret;
    let mut client = connect_authenticated_client(&resolved_config).await?;
    client.refresh_capabilities().await?;
    examine_selected_mailbox(&mut client, mailbox_name).await?;
    Ok(client)
}

fn imap_idle_notification(account_id: AccountId, received_at: String) -> PushNotification {
    PushNotification {
        account_id,
        changed: Vec::new(),
        received_at,
        checkpoint: None,
    }
}

#[cfg(test)]
mod tests;
