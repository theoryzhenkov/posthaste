use std::time::Instant;

use jmap_client::client::Client;
use jmap_client::email;
use posthaste_domain_model::{
    now_iso8601 as domain_now_iso8601, GatewayError, MessageId, MessageRecord, SyncBatch,
    SyncCursor, SyncObject,
};
use posthaste_domain_service::SyncChunkSink;
use posthaste_observability::{events, ph_debug, ph_info, ph_warn};

use crate::conversions::to_message_record;
use crate::live::map_gateway_error;

use super::cursor::{decode_email_cursor_state, encode_email_cursor_state};
use super::MessageSync;

/// Adapts a [`SyncChunkSink`] so [`fetch_email_full_streamed`] just calls
/// `emit` per page (D63/M23b: `emit` is `async`, so the streaming callers pass
/// the real sink directly rather than through a synchronous `FnMut` — see
/// [`fetch_email_full`], which passes this in-memory accumulator instead of a
/// real sink to collect the full snapshot as one batch).
struct AccumulatingSink {
    messages: Vec<MessageRecord>,
}

#[async_trait::async_trait]
impl SyncChunkSink for AccumulatingSink {
    async fn emit(&mut self, batch: SyncBatch) -> Result<(), GatewayError> {
        self.messages.extend(batch.messages);
        Ok(())
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

/// Outcome of a streamed email sync, mirroring [`fetch_email_sync`] but emitting
/// full-snapshot metadata page by page through the caller's [`SyncChunkSink`]
/// so mail surfaces progressively.
///
pub(crate) enum StreamedEmailSync {
    /// Delta sync: small and self-reconciling. The caller emits it as one chunk
    /// carrying its explicit removals and cursors; no reconciliation pass runs.
    Delta(MessageSync),
    /// Full snapshot: metadata pages were already emitted via the sink. Carries
    /// the remote id set and final cursor for the reconciliation pass.
    ///
    /// `remote_ids_complete` is `true` only when the paginated `Email/query`
    /// was proven exhaustive (the reported `total` was reached, or the server
    /// returned a short/empty tail page). When `false`, the server capped the
    /// query and we could NOT prove the id set is the full remote truth
    /// (DS1 mail-loss guard): the caller must upsert what arrived but MUST NOT
    /// prune-by-absence this cycle, or it would durably delete local mail that
    /// still exists remotely beyond the cap.
    FullStreamed {
        remote_message_ids: Vec<MessageId>,
        remote_ids_complete: bool,
        cursor: SyncCursor,
    },
}

/// Streaming counterpart to [`fetch_email_sync`]: a delta returns its batch for
/// the caller to emit as one chunk, while a full snapshot streams metadata pages
/// directly through `sink.emit` (D63/M23b: `emit` is `async` and offloads the
/// store write to the blocking pool, so this takes the sink by reference and
/// awaits it per page rather than going through a synchronous callback) and
/// reports the complete remote id set for reconciliation.
///
pub(crate) async fn fetch_email_sync_streamed(
    client: &Client,
    since_state: Option<&str>,
    sink: &mut dyn SyncChunkSink,
) -> Result<StreamedEmailSync, GatewayError> {
    match since_state.and_then(decode_email_cursor_state) {
        Some(state) => match fetch_email_delta(client, &state).await {
            Ok(sync) => Ok(StreamedEmailSync::Delta(sync)),
            Err(GatewayError::CannotCalculateChanges) => {
                ph_warn!(
                    events::JMAP_EMAIL_DELTA_UNAVAILABLE,
                    "JMAP email delta unavailable, falling back to full snapshot"
                );
                let (remote_message_ids, remote_ids_complete, cursor) =
                    fetch_email_full_streamed(client, sink).await?;
                Ok(StreamedEmailSync::FullStreamed {
                    remote_message_ids,
                    remote_ids_complete,
                    cursor,
                })
            }
            Err(err) => Err(err),
        },
        None => {
            let (remote_message_ids, remote_ids_complete, cursor) =
                fetch_email_full_streamed(client, sink).await?;
            Ok(StreamedEmailSync::FullStreamed {
                remote_message_ids,
                remote_ids_complete,
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
    let mut sink = AccumulatingSink {
        messages: Vec::new(),
    };
    let (_remote_message_ids, remote_ids_complete, cursor) =
        fetch_email_full_streamed(client, &mut sink).await?;
    Ok(MessageSync {
        messages: sink.messages,
        deleted_message_ids: Vec::new(),
        // DS1 mail-loss guard: a full snapshot only earns `replace_all_messages`
        // (which drives prune-by-absence) when the remote id set was proven
        // exhaustive. A capped/incomplete query upserts what it got but never
        // prunes, so mail beyond the server cap is not deleted locally.
        replace_all_messages: remote_ids_complete,
        cursor,
    })
}

/// Full email snapshot streamed page by page: queries all email IDs sorted by
/// `receivedAt DESC`, fetches metadata in chunks of 100, and emits each page to
/// `sink` as it arrives. Returns the complete remote id set and the final
/// cursor for the reconciliation pass. Bodies are omitted (fetched lazily).
///
/// DP-C2 change-consistency: the delta cursor is anchored to the Email object
/// `state` captured BEFORE the first `Email/query` (a get-state anchor), not to
/// the `Email/get` state observed afterwards. Because the cursor predates the
/// query window, any message that arrived or changed DURING pagination is
/// replayed by the next `Email/changes` delta rather than silently skipped
/// (the "invisible new mail" half of DP-C2). The id-set consistency across
/// pages is enforced separately by [`fetch_all_remote_email_ids`] via the
/// per-page `queryState` guard.
///
/// @spec docs/L1-sync#sync-granularity
async fn fetch_email_full_streamed(
    client: &Client,
    sink: &mut dyn SyncChunkSink,
) -> Result<(Vec<MessageId>, bool, SyncCursor), GatewayError> {
    let started = Instant::now();
    // DP-C2: capture the Email object state BEFORE paginating `Email/query`, so
    // the cursor is anchored to a point that precedes the entire snapshot window.
    let state_before = fetch_email_state(client).await?;
    let (email_ids, remote_ids_complete) = fetch_all_remote_email_ids(client).await?;
    ph_info!(
        events::JMAP_EMAIL_FULL_IDS_FETCHED,
        message_count = email_ids.len(),
        remote_ids_complete,
        "JMAP full email snapshot IDs fetched"
    );
    if !remote_ids_complete {
        // DS1/DP-C2 mail-loss guard: either the server capped the query and we
        // could not prove the id set is the full remote truth, or the query
        // result mutated mid-pagination (a `queryState` shift from a concurrent
        // expunge/new mail) so the paginated set is not change-consistent.
        // Upsert what we retrieved (below) but the caller MUST NOT prune-by-
        // absence against this set.
        ph_warn!(
            events::JMAP_EMAIL_FULL_QUERY_INCOMPLETE,
            message_count = email_ids.len(),
            "JMAP full email snapshot query could not be proven complete/consistent; \
             skipping prune-by-absence this cycle"
        );
    }
    let remote_message_ids: Vec<MessageId> = email_ids.iter().cloned().map(MessageId).collect();
    if !email_ids.is_empty() {
        let chunk_count = email_ids.len().div_ceil(100);
        let mut fetched_count = 0usize;
        for (chunk_index, chunk) in email_ids.chunks(100).enumerate() {
            let mut request = client.build();
            request
                .get_email()
                .ids(chunk.iter().map(String::as_str))
                .properties(email_metadata_properties());
            let mut response = request.send_get_email().await.map_err(map_gateway_error)?;
            let page: Vec<MessageRecord> =
                response.take_list().iter().map(to_message_record).collect();
            fetched_count += page.len();
            sink.emit(SyncBatch {
                messages: page,
                ..SyncBatch::default()
            })
            .await?;
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
        remote_ids_complete,
        SyncCursor {
            object_type: SyncObject::Message,
            state: encode_email_cursor_state(&state_before),
            updated_at: domain_now_iso8601().map_err(GatewayError::Rejected)?,
        },
    ))
}

/// Fetch the Email object `state` token with a zero-id `Email/get`. Used as the
/// DP-C2 pre-pagination cursor anchor: capturing it before the first
/// `Email/query` guarantees the next delta replays everything that changed
/// during the snapshot window, so mail arriving mid-pagination is never lost.
async fn fetch_email_state(client: &Client) -> Result<String, GatewayError> {
    let mut request = client.build();
    request.get_email().ids(std::iter::empty::<&str>());
    Ok(request
        .send_get_email()
        .await
        .map_err(map_gateway_error)?
        .take_state())
}

/// Page size requested per `Email/query` when assembling the full-snapshot
/// remote id set. RFC 8620 §5.5 lets a server return fewer ids than requested
/// (Fastmail and some proxies cap the result), so this is only an upper bound
/// on one page; [`fetch_all_remote_email_ids`] pages to exhaustion regardless.
const FULL_SNAPSHOT_EMAIL_QUERY_PAGE_SIZE: usize = 5000;

/// Fetch the COMPLETE set of remote email ids by paging `Email/query`
/// (`receivedAt DESC`) to exhaustion, mirroring the shape of the delta path's
/// `has_more_changes` loop.
///
/// Returns the accumulated ids and whether the set is PROVABLY complete AND
/// change-consistent (DS1 + DP-C2 mail-loss guards). Two failure modes are
/// defended here:
///
///   - **DS1 (capped query):** RFC 8620 §5.5 permits a server to cap the result,
///     and prune-by-absence against a capped set durably deletes every local
///     message beyond the cap though it still exists remotely. We request
///     `calculateTotal` and page to exhaustion, only declaring `complete` when
///     the accumulated count reaches the reported `total` or a short/empty tail
///     page proves we reached the end.
///
///   - **DP-C2 (mid-pagination mutation):** a concurrent server-side expunge or
///     new delivery shifts ids across a page boundary, so a position-paginated
///     set can skip a still-live id (durable loss) or miss new mail while still
///     *looking* complete. We defend this two ways: (a) page by ANCHOR id
///     (`anchor` = last id seen, `anchorOffset` = 1) rather than by numeric
///     position, so the window is pinned to a stable id instead of a count that
///     a concurrent expunge silently shifts; and (b) compare the `queryState`
///     returned on every page against the first page's — any change means the
///     query result mutated mid-pagination, so the accumulated set is NOT a
///     consistent snapshot and we report `complete = false` (withhold prune)
///     rather than prune against a torn set. RFC 8620 §5.5 mandates a stable
///     `queryState` for an unchanged result, so this is the spec-blessed signal.
///
/// The completion terminators are otherwise:
///   - the accumulated count reaches the server-reported `total` (complete), or
///   - the server returns an empty or short (< applied `limit`) tail page
///     (complete — that is the end of the result set), or
///   - the server returns only already-seen ids, i.e. it is not honoring the
///     anchor and cannot be paged (INCOMPLETE — refuse to prune), or
///   - the `queryState` changed across pages (INCOMPLETE — refuse to prune).
///
/// When `total` is known it is authoritative: `complete` is `ids.len() >= total`.
async fn fetch_all_remote_email_ids(client: &Client) -> Result<(Vec<String>, bool), GatewayError> {
    let mut ids: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    // DP-C2: page by anchor id, not numeric position. `None` on the first page
    // (start at the head via `position(0)`); thereafter the last id we saw.
    let mut anchor: Option<String> = None;
    let mut first_query_state: Option<String> = None;
    let mut reported_total: Option<usize> = None;
    let complete: bool;
    loop {
        let mut request = client.build();
        {
            let query = request.query_email();
            query.sort([email::query::Comparator::received_at().descending()]);
            match &anchor {
                // Anchor the window to the last id retrieved and start just past
                // it, so a concurrent expunge cannot silently shift the window.
                Some(anchor_id) => {
                    query.anchor(anchor_id.clone());
                    query.anchor_offset(1);
                }
                None => {
                    query.position(0);
                }
            }
            query.limit(FULL_SNAPSHOT_EMAIL_QUERY_PAGE_SIZE);
            query.calculate_total(true);
        }
        let mut response = request
            .send_query_email()
            .await
            .map_err(map_gateway_error)?;

        // DP-C2 change-consistency guard: the query result must not mutate
        // across pages. A `queryState` shift means a concurrent expunge/new
        // delivery changed the set, so the accumulated ids are not a coherent
        // snapshot — refuse to prune against them this cycle.
        let page_query_state = response.take_query_state();
        match &first_query_state {
            None => first_query_state = Some(page_query_state),
            Some(first) if *first != page_query_state => {
                ph_warn!(
                    events::JMAP_EMAIL_FULL_QUERY_INCOMPLETE,
                    ids_so_far = ids.len(),
                    "JMAP Email/query queryState changed mid-pagination \
                     (concurrent expunge/new mail); snapshot not change-consistent, \
                     withholding prune-by-absence this cycle"
                );
                complete = false;
                break;
            }
            Some(_) => {}
        }

        if let Some(total) = response.total() {
            reported_total = Some(total);
        }
        let applied_limit = response.limit();
        let page = response.take_ids();
        let page_len = page.len();
        let page_last = page.last().cloned();
        let mut new_in_page = 0usize;
        for id in page {
            if seen.insert(id.clone()) {
                ids.push(id);
                new_in_page += 1;
            }
        }

        // Authoritative completion: the server told us the total and we have it.
        if let Some(total) = reported_total {
            if ids.len() >= total {
                complete = true;
                break;
            }
        }
        // Empty page: nothing beyond this anchor. Complete unless a known total
        // says we are still short (server stopped early → not complete).
        if page_len == 0 {
            complete = reported_total.is_none_or(|total| ids.len() >= total);
            break;
        }
        // The server returned only ids we have already seen: it is ignoring the
        // anchor and cannot be paged. We cannot prove completeness.
        if new_in_page == 0 {
            complete = false;
            break;
        }
        // A page shorter than the server's applied limit (or, absent that, our
        // requested page size) is the tail of the result set.
        let effective_limit = applied_limit.unwrap_or(FULL_SNAPSHOT_EMAIL_QUERY_PAGE_SIZE);
        if page_len < effective_limit {
            complete = reported_total.is_none_or(|total| ids.len() >= total);
            break;
        }
        // Advance the window to just past the last id the server returned.
        anchor = page_last;
    }
    Ok((ids, complete))
}

pub(crate) fn email_metadata_properties() -> [email::Property; 19] {
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
        // RFC 2369/8058 unsubscribe targets, fetched raw (`asRaw`) so encoded-
        // word decoding can never mangle a URL; the shared parser unfolds.
        email::Property::Header(email::Header::as_raw("List-Unsubscribe", false)),
        email::Property::Header(email::Header::as_raw("List-Unsubscribe-Post", false)),
    ]
}
