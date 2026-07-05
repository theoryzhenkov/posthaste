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
/// Sets `replace_all_mailboxes = true` (so the store prunes stale local
/// mailboxes) ONLY when the `Mailbox/query` was paginated to exhaustion and
/// proven complete. DP-C3 mail-loss guard: a single unpaginated `Mailbox/query`
/// is the original DS1 bug shape one object type over — RFC 8620 §5.5 lets a
/// server cap the result, and prune-by-absence against a capped/transiently-empty
/// listing would delete EVERY local mailbox (a membership cascade that makes
/// messages unreachable and forces IMAP into a full re-sync). When the listing
/// cannot be proven complete we still upsert what arrived but do NOT drive
/// prune-by-absence this cycle.
///
/// @spec docs/L1-sync#full-snapshot-reconciliation
/// @spec docs/L0-sync#full-snapshot-reconciliation
async fn fetch_mailbox_full(client: &Client) -> Result<MailboxSync, GatewayError> {
    let started = Instant::now();
    let (mailbox_ids, remote_ids_complete) = fetch_all_remote_mailbox_ids(client).await?;
    ph_info!(
        events::JMAP_MAILBOX_FULL_IDS_FETCHED,
        mailbox_count = mailbox_ids.len(),
        remote_ids_complete,
        "JMAP full mailbox snapshot IDs fetched"
    );
    if !remote_ids_complete {
        ph_warn!(
            events::JMAP_MAILBOX_FULL_QUERY_INCOMPLETE,
            mailbox_count = mailbox_ids.len(),
            "JMAP full mailbox snapshot query could not be proven complete; \
             skipping mailbox prune-by-absence this cycle"
        );
    }
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
        // Only an exhaustively-paginated listing earns prune-by-absence.
        replace_all_mailboxes: remote_ids_complete,
        cursor: SyncCursor {
            object_type: SyncObject::Mailbox,
            state,
            updated_at: domain_now_iso8601().map_err(GatewayError::Rejected)?,
        },
    })
}

/// Page size requested per `Mailbox/query` when assembling the full-snapshot
/// remote id set. Mailboxes rarely exceed a few dozen, but RFC 8620 §5.5 still
/// lets a server cap the result, so [`fetch_all_remote_mailbox_ids`] pages to
/// exhaustion regardless.
const FULL_SNAPSHOT_MAILBOX_QUERY_PAGE_SIZE: usize = 1000;

/// Fetch the COMPLETE set of remote mailbox ids by paging `Mailbox/query`
/// (sorted by name for a stable `position` walk) to exhaustion, mirroring
/// [`super::email::fetch_all_remote_email_ids`].
///
/// Returns the accumulated ids and whether the set is PROVABLY complete (DP-C3
/// mail-loss guard): a capped/transiently-empty result must never drive a
/// mailbox prune-by-absence. Completion holds when the accumulated count reaches
/// the server-reported `total`, or the server returns an empty/short tail page;
/// a server that ignores `position` (only ever returns already-seen ids) is
/// INCOMPLETE and refuses to prune.
async fn fetch_all_remote_mailbox_ids(
    client: &Client,
) -> Result<(Vec<String>, bool), GatewayError> {
    let mut ids: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut position: i32 = 0;
    let mut reported_total: Option<usize> = None;
    let complete: bool;
    loop {
        let mut request = client.build();
        {
            let query = request.query_mailbox();
            query.sort([mailbox::query::Comparator::name()]);
            query.position(position);
            query.limit(FULL_SNAPSHOT_MAILBOX_QUERY_PAGE_SIZE);
            query.calculate_total(true);
        }
        let mut response = request
            .send_query_mailbox()
            .await
            .map_err(map_gateway_error)?;
        if let Some(total) = response.total() {
            reported_total = Some(total);
        }
        let applied_limit = response.limit();
        let page = response.take_ids();
        let page_len = page.len();
        let mut new_in_page = 0usize;
        for id in page {
            if seen.insert(id.clone()) {
                ids.push(id);
                new_in_page += 1;
            }
        }

        if let Some(total) = reported_total {
            if ids.len() >= total {
                complete = true;
                break;
            }
        }
        if page_len == 0 {
            complete = reported_total.map_or(true, |total| ids.len() >= total);
            break;
        }
        if new_in_page == 0 {
            complete = false;
            break;
        }
        let effective_limit = applied_limit.unwrap_or(FULL_SNAPSHOT_MAILBOX_QUERY_PAGE_SIZE);
        if page_len < effective_limit {
            complete = reported_total.map_or(true, |total| ids.len() >= total);
            break;
        }
        position = position.checked_add(page_len as i32).ok_or_else(|| {
            GatewayError::Rejected("Mailbox/query position overflow while paginating".to_string())
        })?;
    }
    Ok((ids, complete))
}
