use std::time::Instant;

use jmap_client::client::Client;
use jmap_client::mailbox;
use posthaste_domain_model::{
    now_iso8601 as domain_now_iso8601, GatewayError, MailboxId, SyncCursor, SyncObject,
};
use posthaste_observability::{events, ph_debug, ph_info, ph_warn};

use crate::conversions::to_mailbox_record;
use crate::live::map_gateway_error;

use super::cursor::non_empty_state;
use super::MailboxSync;

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
