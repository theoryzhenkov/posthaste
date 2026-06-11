use std::time::Instant;

use jmap_client::client::Client;
use jmap_client::{email, mailbox};
use posthaste_domain::{
    now_iso8601 as domain_now_iso8601, GatewayError, MailboxId, MailboxRecord, MessageId,
    MessageRecord, SyncCursor, SyncObject,
};
use posthaste_observability::{events, ph_debug, ph_info, ph_warn};
use serde_json::json;

use crate::conversions::{to_mailbox_record, to_message_record};
use crate::live::map_gateway_error;

const EMAIL_CURSOR_KIND: &str = "jmap-email";
const EMAIL_METADATA_VERSION: u64 = 2;

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
    /// When `true`, the store treats this as an authoritative snapshot and
    /// prunes any local messages missing from the result.
    pub replace_all_messages: bool,
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
    match since_state.and_then(non_empty_state) {
        Some(state) => match fetch_mailbox_delta(client, state).await {
            Ok(sync) => Ok(sync),
            Err(GatewayError::CannotCalculateChanges) => {
                ph_warn!(
                    events::JMAP_MAILBOX_DELTA_UNAVAILABLE,
                    "JMAP mailbox delta unavailable, falling back to full snapshot"
                );
                fetch_mailbox_full(client).await
            }
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
    match since_state.and_then(decode_email_cursor_state) {
        Some(state) => match fetch_email_delta(client, &state).await {
            Ok(sync) => Ok(sync),
            Err(GatewayError::CannotCalculateChanges) => {
                ph_warn!(
                    events::JMAP_EMAIL_DELTA_UNAVAILABLE,
                    "JMAP email delta unavailable, falling back to full snapshot"
                );
                fetch_email_full(client).await
            }
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
    ph_debug!(
        events::JMAP_MAILBOX_DELTA_STARTED,
        "JMAP mailbox delta fetch started"
    );
    let mut current_state = since_state.to_string();
    let mut upsert = Vec::new();
    let mut deleted = Vec::new();
    let mut page_count = 0usize;
    loop {
        let changes = client
            .mailbox_changes(&current_state, 500)
            .await
            .map_err(map_gateway_error)?;
        page_count += 1;
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
    ph_debug!(
        events::JMAP_MAILBOX_DELTA_COMPLETED,
        page_count,
        upsert_count = upsert.len(),
        deleted_count = deleted.len(),
        "JMAP mailbox delta fetch completed"
    );
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
    ph_debug!(
        events::JMAP_EMAIL_DELTA_STARTED,
        "JMAP email delta fetch started"
    );
    let mut current_state = since_state.to_string();
    let mut upsert = Vec::new();
    let mut deleted = Vec::new();
    let mut page_count = 0usize;
    let mut chunk_count = 0usize;
    loop {
        let changes = client
            .email_changes(&current_state, Some(500))
            .await
            .map_err(map_gateway_error)?;
        page_count += 1;
        deleted.extend(changes.destroyed().iter().cloned().map(MessageId));
        let fetch_ids: Vec<String> = changes
            .created()
            .iter()
            .chain(changes.updated().iter())
            .cloned()
            .collect();
        for chunk in fetch_ids.chunks(100) {
            chunk_count += 1;
            let mut request = client.build();
            request
                .get_email()
                .ids(chunk.iter().map(String::as_str))
                .properties(email_metadata_properties());
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
    ph_debug!(
        events::JMAP_EMAIL_DELTA_COMPLETED,
        page_count,
        chunk_count,
        upsert_count = upsert.len(),
        deleted_count = deleted.len(),
        "JMAP email delta fetch completed"
    );
    Ok(MessageSync {
        messages: upsert,
        deleted_message_ids: deleted,
        replace_all_messages: false,
        cursor: SyncCursor {
            object_type: SyncObject::Message,
            state: encode_email_cursor_state(&current_state),
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
    let started = Instant::now();
    let mailbox_ids = client
        .mailbox_query(None::<mailbox::query::Filter>, None::<Vec<_>>)
        .await
        .map_err(map_gateway_error)?
        .take_ids();
    ph_info!(
        events::JMAP_MAILBOX_FULL_IDS_FETCHED,
        mailbox_count = mailbox_ids.len(),
        "JMAP full mailbox snapshot IDs fetched"
    );
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
    ph_info!(
        events::JMAP_MAILBOX_FULL_SNAPSHOT_FETCHED,
        mailbox_count = mailbox_ids.len(),
        duration_ms = started.elapsed().as_millis() as u64,
        "JMAP full mailbox snapshot fetched"
    );
    Ok(MailboxSync {
        mailboxes: response.take_list().iter().map(to_mailbox_record).collect(),
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
    let started = Instant::now();
    let email_ids = client
        .email_query(
            None::<email::query::Filter>,
            [email::query::Comparator::received_at().descending()].into(),
        )
        .await
        .map_err(map_gateway_error)?
        .take_ids();
    ph_info!(
        events::JMAP_EMAIL_FULL_IDS_FETCHED,
        message_count = email_ids.len(),
        "JMAP full email snapshot IDs fetched"
    );
    let mut messages = Vec::new();
    let mut state = None;
    if email_ids.is_empty() {
        let mut request = client.build();
        request.get_email().ids(std::iter::empty::<&str>());
        state = Some(
            request
                .send_get_email()
                .await
                .map_err(map_gateway_error)?
                .take_state(),
        );
    } else {
        let chunk_count = email_ids.len().div_ceil(100);
        for (chunk_index, chunk) in email_ids.chunks(100).enumerate() {
            let mut request = client.build();
            request
                .get_email()
                .ids(chunk.iter().map(String::as_str))
                .properties(email_metadata_properties());
            let mut response = request.send_get_email().await.map_err(map_gateway_error)?;
            if state.is_none() {
                state = Some(response.take_state());
            }
            messages.extend(response.take_list().iter().map(to_message_record));
            ph_info!(
                events::JMAP_EMAIL_FULL_METADATA_PROGRESS,
                chunk_index = chunk_index + 1,
                chunk_count,
                fetched_count = messages.len(),
                total_count = email_ids.len(),
                "JMAP full email metadata fetch progress"
            );
        }
    }
    ph_info!(
        events::JMAP_EMAIL_FULL_SNAPSHOT_FETCHED,
        message_count = messages.len(),
        duration_ms = started.elapsed().as_millis() as u64,
        "JMAP full email snapshot fetched"
    );
    Ok(MessageSync {
        messages,
        deleted_message_ids: Vec::new(),
        replace_all_messages: true,
        cursor: SyncCursor {
            object_type: SyncObject::Message,
            state: encode_email_cursor_state(&state.unwrap_or_default()),
            updated_at: domain_now_iso8601().map_err(GatewayError::Rejected)?,
        },
    })
}

fn non_empty_state(state: &str) -> Option<&str> {
    (!state.is_empty()).then_some(state)
}

pub(crate) fn encode_email_cursor_state(server_state: &str) -> String {
    json!({
        "kind": EMAIL_CURSOR_KIND,
        "metadataVersion": EMAIL_METADATA_VERSION,
        "state": server_state,
    })
    .to_string()
}

pub(crate) fn decode_email_cursor_state(state: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(state).ok()?;
    let kind = value.get("kind")?.as_str()?;
    let metadata_version = value.get("metadataVersion")?.as_u64()?;
    if kind != EMAIL_CURSOR_KIND || metadata_version != EMAIL_METADATA_VERSION {
        return None;
    }
    value
        .get("state")?
        .as_str()
        .and_then(non_empty_state)
        .map(String::from)
}

fn email_metadata_properties() -> [email::Property; 16] {
    [
        email::Property::Id,
        email::Property::ThreadId,
        email::Property::BlobId,
        email::Property::MailboxIds,
        email::Property::Keywords,
        email::Property::Subject,
        email::Property::From,
        email::Property::To,
        email::Property::Preview,
        email::Property::ReceivedAt,
        email::Property::SentAt,
        email::Property::HasAttachment,
        email::Property::Size,
        email::Property::MessageId,
        email::Property::References,
        email::Property::InReplyTo,
    ]
}

#[cfg(test)]
mod tests;
