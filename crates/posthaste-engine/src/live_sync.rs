use std::sync::Arc;
use std::time::Instant;

use jmap_client::client::Client;
use posthaste_domain::{GatewayError, SyncBatch, SyncCursor, SyncObject};
use tracing::info;

use crate::sync::{fetch_email_sync, fetch_mailbox_sync};

/// Perform a full sync cycle: mailbox state then email state.
///
/// Falls back from delta to full sync on `cannotCalculateChanges`.
///
/// @spec docs/L1-sync#sync-loop
/// @spec docs/L1-sync#state-management
pub(crate) async fn sync_account(
    client: &Arc<Client>,
    cursors: &[SyncCursor],
) -> Result<SyncBatch, GatewayError> {
    let mailbox_cursor = cursors
        .iter()
        .find(|cursor| cursor.object_type == SyncObject::Mailbox)
        .map(|cursor| cursor.state.as_str());
    let message_cursor = cursors
        .iter()
        .find(|cursor| cursor.object_type == SyncObject::Message)
        .map(|cursor| cursor.state.as_str());

    info!(
        has_mailbox_state = mailbox_cursor.is_some(),
        has_message_state = message_cursor.is_some(),
        "JMAP sync fetch started"
    );
    let mailbox_start = Instant::now();
    let mailbox_sync = fetch_mailbox_sync(client, mailbox_cursor).await?;
    info!(
        mode = if mailbox_sync.replace_all_mailboxes {
            "full"
        } else {
            "delta"
        },
        mailbox_count = mailbox_sync.mailboxes.len(),
        deleted_mailbox_count = mailbox_sync.deleted_mailbox_ids.len(),
        duration_ms = mailbox_start.elapsed().as_millis() as u64,
        "JMAP mailbox sync fetched"
    );
    let email_start = Instant::now();
    let email_sync = fetch_email_sync(client, message_cursor).await?;
    info!(
        mailboxes = mailbox_sync.mailboxes.len(),
        messages = email_sync.messages.len(),
        deleted_mailboxes = mailbox_sync.deleted_mailbox_ids.len(),
        deleted_messages = email_sync.deleted_message_ids.len(),
        replace_all_mailboxes = mailbox_sync.replace_all_mailboxes,
        replace_all_messages = email_sync.replace_all_messages,
        email_duration_ms = email_start.elapsed().as_millis() as u64,
        "JMAP sync batch fetched"
    );

    Ok(SyncBatch {
        mailboxes: mailbox_sync.mailboxes,
        messages: email_sync.messages,
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_mailbox_ids: mailbox_sync.deleted_mailbox_ids,
        deleted_message_ids: email_sync.deleted_message_ids,
        replace_all_mailboxes: mailbox_sync.replace_all_mailboxes,
        replace_all_messages: email_sync.replace_all_messages,
        cursors: vec![mailbox_sync.cursor, email_sync.cursor],
    })
}
