use std::time::Duration;

use async_stream::stream;
use posthaste_domain::{
    now_iso8601, AccountId, PushEventStream, PushNotification, PushStreamEvent,
};
use tracing::{debug, warn};

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
) -> PushEventStream {
    Box::pin(stream! {
        loop {
            match connect_idle_client(&config, &mailbox_name).await {
                Ok(mut client) => {
                    yield PushStreamEvent::Connected {
                        transport: "imap-idle",
                    };

                    loop {
                        let tag = client.enqueue_idle();
                        match client.idle(tag).await {
                            Ok(()) => {
                                debug!(account_id = %account_id, mailbox_name, "IMAP IDLE returned");
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
                                yield PushStreamEvent::Notification(PushNotification {
                                    account_id: account_id.clone(),
                                    changed: vec![format!("imap:{mailbox_name}")],
                                    received_at,
                                    checkpoint: None,
                                });
                            }
                            Err(error) => {
                                warn!(
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
                    warn!(
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
) -> Result<imap_client::client::tokio::Client, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    client.refresh_capabilities().await?;
    examine_selected_mailbox(&mut client, mailbox_name).await?;
    Ok(client)
}
