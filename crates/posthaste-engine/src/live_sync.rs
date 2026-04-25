use std::sync::Arc;

use jmap_client::client::Client;
use posthaste_domain::{GatewayError, SyncBatch, SyncCursor, SyncObject};
use tracing::debug;

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

    debug!(
        has_mailbox_state = mailbox_cursor.is_some(),
        has_message_state = message_cursor.is_some(),
        "fetching JMAP changes"
    );
    let mailbox_sync = fetch_mailbox_sync(client, mailbox_cursor).await?;
    let email_sync = fetch_email_sync(client, message_cursor).await?;
    debug!(
        mailboxes = mailbox_sync.mailboxes.len(),
        messages = email_sync.messages.len(),
        deleted_mailboxes = mailbox_sync.deleted_mailbox_ids.len(),
        deleted_messages = email_sync.deleted_message_ids.len(),
        replace_all_mailboxes = mailbox_sync.replace_all_mailboxes,
        "JMAP sync batch fetched"
    );

    Ok(SyncBatch {
        mailboxes: mailbox_sync.mailboxes,
        messages: email_sync.messages,
        imap_message_locations: Vec::new(),
        deleted_mailbox_ids: mailbox_sync.deleted_mailbox_ids,
        deleted_message_ids: email_sync.deleted_message_ids,
        replace_all_mailboxes: mailbox_sync.replace_all_mailboxes,
        replace_all_messages: email_sync.replace_all_messages,
        cursors: vec![mailbox_sync.cursor, email_sync.cursor],
    })
}
