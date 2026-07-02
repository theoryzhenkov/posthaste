use std::time::Instant;

use jmap_client::client::Client;
use jmap_client::email;
use posthaste_domain_model::{
    now_iso8601 as domain_now_iso8601, GatewayError, MessageId, MessageRecord, SyncCursor,
    SyncObject,
};
use posthaste_observability::{events, ph_debug, ph_info, ph_warn};

use crate::conversions::to_message_record;
use crate::live::map_gateway_error;

use super::cursor::{decode_email_cursor_state, encode_email_cursor_state};
use super::MessageSync;

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

/// Outcome of a streamed email sync, mirroring [`fetch_email_sync`] but emitting
/// full-snapshot metadata page by page through `on_page` so mail surfaces
/// progressively.
///
pub(crate) enum StreamedEmailSync {
    /// Delta sync: small and self-reconciling. The caller emits it as one chunk
    /// carrying its explicit removals and cursors; no reconciliation pass runs.
    Delta(MessageSync),
    /// Full snapshot: metadata pages were already emitted via `on_page`. Carries
    /// the complete remote id set and final cursor for the reconciliation pass.
    FullStreamed {
        remote_message_ids: Vec<MessageId>,
        cursor: SyncCursor,
    },
}

/// Streaming counterpart to [`fetch_email_sync`]: a delta returns its batch for
/// the caller to emit as one chunk, while a full snapshot streams metadata pages
/// through `on_page` and reports the complete remote id set for reconciliation.
///
pub(crate) async fn fetch_email_sync_streamed(
    client: &Client,
    since_state: Option<&str>,
    on_page: &mut (dyn FnMut(Vec<MessageRecord>) -> Result<(), GatewayError> + Send),
) -> Result<StreamedEmailSync, GatewayError> {
    match since_state.and_then(decode_email_cursor_state) {
        Some(state) => match fetch_email_delta(client, &state).await {
            Ok(sync) => Ok(StreamedEmailSync::Delta(sync)),
            Err(GatewayError::CannotCalculateChanges) => {
                ph_warn!(
                    events::JMAP_EMAIL_DELTA_UNAVAILABLE,
                    "JMAP email delta unavailable, falling back to full snapshot"
                );
                let (remote_message_ids, cursor) =
                    fetch_email_full_streamed(client, on_page).await?;
                Ok(StreamedEmailSync::FullStreamed {
                    remote_message_ids,
                    cursor,
                })
            }
            Err(err) => Err(err),
        },
        None => {
            let (remote_message_ids, cursor) = fetch_email_full_streamed(client, on_page).await?;
            Ok(StreamedEmailSync::FullStreamed {
                remote_message_ids,
                cursor,
            })
        }
    }
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

/// Full email snapshot via `Email/query` + `Email/get`.
///
/// Queries all email IDs sorted by `receivedAt DESC` and fetches metadata
/// in chunks of 100. Bodies are omitted (fetched lazily on first view).
///
/// @spec docs/L1-sync#sync-granularity
/// @spec docs/L0-sync#sync-granularity
async fn fetch_email_full(client: &Client) -> Result<MessageSync, GatewayError> {
    let mut messages = Vec::new();
    let (_remote_message_ids, cursor) = fetch_email_full_streamed(client, &mut |page| {
        messages.extend(page);
        Ok(())
    })
    .await?;
    Ok(MessageSync {
        messages,
        deleted_message_ids: Vec::new(),
        replace_all_messages: true,
        cursor,
    })
}

/// Full email snapshot streamed page by page: queries all email IDs sorted by
/// `receivedAt DESC`, fetches metadata in chunks of 100, and hands each page to
/// `on_page` as it arrives. Returns the complete remote id set and the final
/// cursor for the reconciliation pass. Bodies are omitted (fetched lazily).
///
/// @spec docs/L1-sync#sync-granularity
async fn fetch_email_full_streamed(
    client: &Client,
    on_page: &mut (dyn FnMut(Vec<MessageRecord>) -> Result<(), GatewayError> + Send),
) -> Result<(Vec<MessageId>, SyncCursor), GatewayError> {
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
    let remote_message_ids: Vec<MessageId> = email_ids.iter().cloned().map(MessageId).collect();
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
        let mut fetched_count = 0usize;
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
            let page: Vec<MessageRecord> =
                response.take_list().iter().map(to_message_record).collect();
            fetched_count += page.len();
            on_page(page)?;
            ph_info!(
                events::JMAP_EMAIL_FULL_METADATA_PROGRESS,
                chunk_index = chunk_index + 1,
                chunk_count,
                fetched_count,
                total_count = email_ids.len(),
                "JMAP full email metadata fetch progress"
            );
        }
    }
    ph_info!(
        events::JMAP_EMAIL_FULL_SNAPSHOT_FETCHED,
        message_count = remote_message_ids.len(),
        duration_ms = started.elapsed().as_millis() as u64,
        "JMAP full email snapshot fetched"
    );
    Ok((
        remote_message_ids,
        SyncCursor {
            object_type: SyncObject::Message,
            state: encode_email_cursor_state(&state.unwrap_or_default()),
            updated_at: domain_now_iso8601().map_err(GatewayError::Rejected)?,
        },
    ))
}

pub(crate) fn email_metadata_properties() -> [email::Property; 17] {
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
        // Stable draft identity round-tripped through the draft's headers.
        email::Property::Header(email::Header::as_text(
            posthaste_domain_model::DRAFT_ID_HEADER,
            false,
        )),
    ]
}
