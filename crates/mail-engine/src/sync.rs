use jmap_client::client::Client;
use jmap_client::{email, mailbox};
use mail_domain::{
    now_iso8601 as domain_now_iso8601, GatewayError, MailboxId, MailboxRecord, MessageId,
    MessageRecord, SyncCursor, SyncObject,
};

use crate::conversions::{to_mailbox_record, to_message_record};
use crate::live::map_gateway_error;

/// Result of a mailbox sync cycle (delta or full).
///
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
pub(crate) struct MailboxSync {
    pub mailboxes: Vec<MailboxRecord>,
    pub deleted_mailbox_ids: Vec<MailboxId>,
    /// When `true`, the store treats this as an authoritative snapshot and
    /// prunes any local mailboxes missing from the result.
    pub replace_all_mailboxes: bool,
    pub cursor: SyncCursor,
}

/// Result of an email sync cycle (delta or full).
///
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
pub(crate) struct MessageSync {
    pub messages: Vec<MessageRecord>,
    pub deleted_message_ids: Vec<MessageId>,
    pub cursor: SyncCursor,
}

/// Sync mailbox state: try delta via `Mailbox/changes`, fall back to full snapshot
/// on `cannotCalculateChanges`.
///
/// @spec docs/L1-sync#state-management
/// @spec docs/L1-sync#error-handling
pub(crate) async fn fetch_mailbox_sync(
    client: &Client,
    since_state: Option<&str>,
) -> Result<MailboxSync, GatewayError> {
    match since_state {
        Some(state) => match fetch_mailbox_delta(client, state).await {
            Ok(sync) => Ok(sync),
            Err(GatewayError::CannotCalculateChanges) => fetch_mailbox_full(client).await,
            Err(err) => Err(err),
        },
        None => fetch_mailbox_full(client).await,
    }
}

/// Sync email state: try delta via `Email/changes`, fall back to full snapshot
/// on `cannotCalculateChanges`.
///
/// @spec docs/L1-sync#state-management
/// @spec docs/L1-sync#error-handling
pub(crate) async fn fetch_email_sync(
    client: &Client,
    since_state: Option<&str>,
) -> Result<MessageSync, GatewayError> {
    match since_state {
        Some(state) => match fetch_email_delta(client, state).await {
            Ok(sync) => Ok(sync),
            Err(GatewayError::CannotCalculateChanges) => fetch_email_full(client).await,
            Err(err) => Err(err),
        },
        None => fetch_email_full(client).await,
    }
}

/// Incremental mailbox sync via `Mailbox/changes` + `Mailbox/get`.
///
/// Loops through paginated change batches until `has_more_changes` is false.
///
/// @spec docs/L1-jmap#methods-used
/// @spec docs/L1-sync#state-management
async fn fetch_mailbox_delta(
    client: &Client,
    since_state: &str,
) -> Result<MailboxSync, GatewayError> {
    let mut current_state = since_state.to_string();
    let mut upsert = Vec::new();
    let mut deleted = Vec::new();
    loop {
        let changes = client
            .mailbox_changes(&current_state, 500)
            .await
            .map_err(map_gateway_error)?;
        deleted.extend(changes.destroyed().iter().cloned().map(MailboxId));
        let fetch_ids: Vec<&str> = changes
            .created()
            .iter()
            .chain(changes.updated().iter())
            .map(String::as_str)
            .collect();
        if !fetch_ids.is_empty() {
            let mut request = client.build();
            request.get_mailbox().ids(fetch_ids).properties([
                mailbox::Property::Id,
                mailbox::Property::Name,
                mailbox::Property::Role,
                mailbox::Property::UnreadEmails,
                mailbox::Property::TotalEmails,
            ]);
            for mailbox in request
                .send_get_mailbox()
                .await
                .map_err(map_gateway_error)?
                .take_list()
            {
                upsert.push(to_mailbox_record(&mailbox));
            }
        }
        current_state = changes.new_state().to_string();
        if !changes.has_more_changes() {
            break;
        }
    }
    Ok(MailboxSync {
        mailboxes: upsert,
        deleted_mailbox_ids: deleted,
        replace_all_mailboxes: false,
        cursor: SyncCursor {
            object_type: SyncObject::Mailbox,
            state: current_state,
            updated_at: domain_now_iso8601().map_err(GatewayError::Rejected)?,
        },
    })
}

/// Incremental email sync via `Email/changes` + `Email/get`.
///
/// Fetches changed email IDs in batches and retrieves their metadata in
/// chunks of 100 to stay within JMAP request size limits.
///
/// @spec docs/L1-jmap#methods-used
/// @spec docs/L1-sync#state-management
async fn fetch_email_delta(
    client: &Client,
    since_state: &str,
) -> Result<MessageSync, GatewayError> {
    let mut current_state = since_state.to_string();
    let mut upsert = Vec::new();
    let mut deleted = Vec::new();
    loop {
        let changes = client
            .email_changes(&current_state, Some(500))
            .await
            .map_err(map_gateway_error)?;
        deleted.extend(changes.destroyed().iter().cloned().map(MessageId));
        let fetch_ids: Vec<String> = changes
            .created()
            .iter()
            .chain(changes.updated().iter())
            .cloned()
            .collect();
        for chunk in fetch_ids.chunks(100) {
            let mut request = client.build();
            request
                .get_email()
                .ids(chunk.iter().map(String::as_str))
                .properties([
                    email::Property::Id,
                    email::Property::ThreadId,
                    email::Property::BlobId,
                    email::Property::MailboxIds,
                    email::Property::Keywords,
                    email::Property::Subject,
                    email::Property::From,
                    email::Property::Preview,
                    email::Property::ReceivedAt,
                    email::Property::HasAttachment,
                    email::Property::Size,
                ]);
            for email in request
                .send_get_email()
                .await
                .map_err(map_gateway_error)?
                .take_list()
            {
                upsert.push(to_message_record(&email));
            }
        }
        current_state = changes.new_state().to_string();
        if !changes.has_more_changes() {
            break;
        }
    }
    Ok(MessageSync {
        messages: upsert,
        deleted_message_ids: deleted,
        cursor: SyncCursor {
            object_type: SyncObject::Message,
            state: current_state,
            updated_at: domain_now_iso8601().map_err(GatewayError::Rejected)?,
        },
    })
}

/// Full mailbox snapshot via `Mailbox/query` + `Mailbox/get`.
///
/// Sets `replace_all_mailboxes = true` so the store prunes stale local
/// mailboxes that no longer exist on the server.
///
/// @spec docs/L1-sync#full-snapshot-reconciliation
/// @spec docs/L0-sync#full-snapshot-reconciliation
async fn fetch_mailbox_full(client: &Client) -> Result<MailboxSync, GatewayError> {
    let mailbox_ids = client
        .mailbox_query(None::<mailbox::query::Filter>, None::<Vec<_>>)
        .await
        .map_err(map_gateway_error)?
        .take_ids();
    let mut request = client.build();
    request
        .get_mailbox()
        .ids(mailbox_ids.iter().map(String::as_str))
        .properties([
            mailbox::Property::Id,
            mailbox::Property::Name,
            mailbox::Property::Role,
            mailbox::Property::UnreadEmails,
            mailbox::Property::TotalEmails,
        ]);
    let mut response = request
        .send_get_mailbox()
        .await
        .map_err(map_gateway_error)?;
    let state = response.take_state();
    Ok(MailboxSync {
        mailboxes: response
            .take_list()
            .iter()
            .map(to_mailbox_record)
            .collect(),
        deleted_mailbox_ids: Vec::new(),
        replace_all_mailboxes: true,
        cursor: SyncCursor {
            object_type: SyncObject::Mailbox,
            state,
            updated_at: domain_now_iso8601().map_err(GatewayError::Rejected)?,
        },
    })
}

/// Full email snapshot via `Email/query` + `Email/get`.
///
/// Queries all email IDs sorted by `receivedAt DESC` and fetches metadata
/// in chunks of 100. Bodies are omitted (fetched lazily on first view).
///
/// @spec docs/L1-sync#sync-granularity
/// @spec docs/L0-sync#sync-granularity
async fn fetch_email_full(client: &Client) -> Result<MessageSync, GatewayError> {
    let email_ids = client
        .email_query(
            None::<email::query::Filter>,
            [email::query::Comparator::received_at().descending()].into(),
        )
        .await
        .map_err(map_gateway_error)?
        .take_ids();
    let mut messages = Vec::new();
    let mut state = None;
    for chunk in email_ids.chunks(100) {
        let mut request = client.build();
        request
            .get_email()
            .ids(chunk.iter().map(String::as_str))
            .properties([
                email::Property::Id,
                email::Property::ThreadId,
                email::Property::BlobId,
                email::Property::MailboxIds,
                email::Property::Keywords,
                email::Property::Subject,
                email::Property::From,
                email::Property::Preview,
                email::Property::ReceivedAt,
                email::Property::HasAttachment,
                email::Property::Size,
                email::Property::MessageId,
                email::Property::References,
                email::Property::InReplyTo,
            ]);
        let mut response = request.send_get_email().await.map_err(map_gateway_error)?;
        if state.is_none() {
            state = Some(response.take_state());
        }
        messages.extend(response.take_list().iter().map(to_message_record));
    }
    Ok(MessageSync {
        messages,
        deleted_message_ids: Vec::new(),
        cursor: SyncCursor {
            object_type: SyncObject::Message,
            state: state.unwrap_or_default(),
            updated_at: domain_now_iso8601().map_err(GatewayError::Rejected)?,
        },
    })
}
