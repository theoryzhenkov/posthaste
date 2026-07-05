use super::*;

/// Streamed sync driver for IMAP (B4): the resumable, interruption-safe
/// counterpart to [`super::sync_imap_account`].
///
/// A large account's INITIAL sync used to accumulate every header in memory and
/// persist them in a SINGLE terminal transaction — an app close at header
/// 80k/100k retained ZERO rows and the next launch restarted from UID 1. This
/// path streams each resumable initial full snapshot in bounded UID chunks,
/// committing every chunk's rows AND an advancing durable cursor
/// (`ImapMailboxSyncState::partial_initial_uid`) via the store's existing
/// per-chunk `apply_sync_batch` machinery (the same shape the JMAP streamed
/// fallback uses). A crash mid-snapshot therefore keeps every committed chunk,
/// and the planner resumes from the cursor instead of refetching from UID 1.
///
/// DS1 interplay: every streamed chunk (including the finalizing one) is
/// UPSERT-ONLY — `replace_all_messages` is never set and no deletions are
/// carried — so a mid-sync checkpoint can never drive prune-by-absence against
/// an incomplete remote set. An initial sync has nothing to prune anyway (a
/// fresh mailbox has no local rows; a resumed one only re-commits its own
/// prefix), so completion is additive, never destructive.
pub(crate) async fn sync_imap_account_streamed(
    gateway: &LiveImapSmtpGateway,
    account_id: &AccountId,
    progress: Option<SyncProgressReporter>,
    sink: &mut dyn SyncChunkSink,
) -> Result<SyncOutcome, GatewayError> {
    let mut lease = gateway
        .sessions
        .acquire("sync_streamed")
        .await
        .map_err(imap_error_to_gateway)?;
    let result =
        sync_imap_account_streamed_with_client(gateway, lease.client(), account_id, progress, sink)
            .await;
    lease.finish_gateway(result)
}

async fn sync_imap_account_streamed_with_client(
    gateway: &LiveImapSmtpGateway,
    client: &mut ImapClient,
    account_id: &AccountId,
    progress: Option<SyncProgressReporter>,
    sink: &mut dyn SyncChunkSink,
) -> Result<SyncOutcome, GatewayError> {
    let PlannedSync {
        discovery,
        planned_mailboxes,
        fetch_modseq,
        fetch_gmail_metadata,
        account_full_message_snapshot,
        updated_at,
        sync_started,
    } = prepare_planned_sync(gateway, client, account_id, &progress).await?;

    // Resumable initial full snapshots stream per-chunk; every other plan
    // (deltas, other full-snapshot reasons, skip-unchanged) stays on the
    // accumulate-then-single-batch path, which carries its own explicit
    // deletions and self-reconciles.
    let (initial, rest): (Vec<PlannedImapMailbox>, Vec<PlannedImapMailbox>) = planned_mailboxes
        .into_iter()
        .partition(is_streaming_initial_snapshot);

    let ctx = MailboxPlanExecutionContext {
        account_id,
        fetch_modseq,
        fetch_gmail_metadata,
        account_full_message_snapshot,
        updated_at: &updated_at,
        progress: &progress,
    };

    if initial.is_empty() {
        // No resumable initial snapshot this cycle: behave exactly like the
        // batch path — one self-reconciling emit.
        let has_full_mailbox_snapshot =
            account_full_message_snapshot || planned_mailboxes_include_full_snapshot(&rest);
        let requires_partial_delta_batch = planned_mailboxes_require_partial_delta_batch(&rest);
        let accumulator = execute_mailbox_plans(client, rest, ctx).await?;
        let batch = accumulator.into_sync_batch(
            account_id,
            discovery,
            account_full_message_snapshot,
            requires_partial_delta_batch,
            has_full_mailbox_snapshot,
            updated_at,
        );
        sink.emit(batch).await?;
        return Ok(SyncOutcome::single_batch());
    }

    ph_info!(
        events::IMAP_SYNC_FETCH_STARTED,
        account_id = %account_id,
        mailbox_count = initial.len() + rest.len(),
        account_full_message_snapshot,
        streaming_initial_count = initial.len(),
        "IMAP streamed sync fetch started"
    );

    // The authoritative mailbox set (upsert + prune absent mailboxes) is emitted
    // once, before any message chunk, so streamed message rows never reference a
    // not-yet-upserted mailbox.
    sink.emit(imap_mailbox_sync_batch(
        account_id,
        discovery.clone(),
        updated_at.clone(),
    ))
    .await?;

    let initial_count = initial.len();
    for (index, mailbox) in initial.iter().enumerate() {
        stream_initial_mailbox_snapshot(
            client,
            mailbox,
            &discovery,
            ctx,
            index + 1,
            initial_count,
            sink,
        )
        .await?;
    }

    // The remaining (delta / other-full) mailboxes accumulate into one
    // self-reconciling message batch. Mailboxes were already emitted above, so
    // strip them here to avoid a redundant replace-all pass.
    if !rest.is_empty() {
        let has_full_mailbox_snapshot = planned_mailboxes_include_full_snapshot(&rest);
        let requires_partial_delta_batch = planned_mailboxes_require_partial_delta_batch(&rest);
        let accumulator = execute_mailbox_plans(client, rest, ctx).await?;
        let mut batch = accumulator.into_sync_batch(
            account_id,
            discovery,
            account_full_message_snapshot,
            requires_partial_delta_batch,
            has_full_mailbox_snapshot,
            updated_at.clone(),
        );
        batch.mailboxes.clear();
        batch.replace_all_mailboxes = false;
        sink.emit(batch).await?;
    }

    ph_info!(
        events::IMAP_SYNC_FETCH_COMPLETED,
        account_id = %account_id,
        mailbox_count = initial_count,
        duration_ms = sync_started.elapsed().as_millis() as u64,
        "IMAP streamed sync fetch completed"
    );

    Ok(SyncOutcome::single_batch())
}

fn is_streaming_initial_snapshot(mailbox: &PlannedImapMailbox) -> bool {
    matches!(
        mailbox.plan,
        PlannedImapMailboxSync::Sync(ImapMailboxSyncPlan::FullSnapshot {
            reason: ImapFullSyncReason::InitialSync | ImapFullSyncReason::ResumeInitialSync,
        })
    )
}

/// Fetch one mailbox's initial full snapshot in ascending-UID chunks, committing
/// each chunk (rows + advancing `partial_initial_uid` cursor) through the sink
/// before fetching the next, then a finalizing chunk that clears the cursor and
/// writes the completed watermark.
///
/// On a fresh initial sync the resume point is UID 0 (fetch everything); on a
/// resumed one it is the stored `partial_initial_uid`, so [`uids_above`] returns
/// only the not-yet-committed tail — nothing already committed is re-fetched and
/// nothing is skipped. Idempotent-by-UID upserts make even an
/// interrupted-then-replayed chunk safe.
async fn stream_initial_mailbox_snapshot(
    client: &mut ImapClient,
    mailbox: &PlannedImapMailbox,
    discovery: &DiscoveredImapAccount,
    ctx: MailboxPlanExecutionContext<'_>,
    mailbox_ordinal: usize,
    mailbox_count: usize,
    sink: &mut dyn SyncChunkSink,
) -> Result<(), GatewayError> {
    let started = Instant::now();
    let after_uid = mailbox
        .stored_state
        .as_ref()
        .and_then(|state| state.partial_initial_uid)
        .unwrap_or(ImapUid(0));
    let resuming = after_uid.0 > 0;

    report_sync_progress(
        ctx.progress,
        ImapSyncProgressUpdate::new(
            SyncProgressStage::Fetching,
            if resuming {
                "Resuming mailbox sync"
            } else {
                "Fetching mailbox"
            },
        )
        .with_mailbox(mailbox.name.clone(), mailbox_ordinal, mailbox_count),
    );
    ph_info!(
        events::IMAP_MAILBOX_HEADER_FETCH_STARTED,
        account_id = %ctx.account_id,
        mailbox_id = %mailbox.id,
        mailbox_index = mailbox_ordinal,
        mailbox_count,
        mode = if resuming { "resume_initial_snapshot" } else { "initial_snapshot" },
        after_uid = after_uid.0,
        "IMAP mailbox header fetch started"
    );

    let (selected, all_uids) = search_undeleted_uids(client, &mailbox.name, ctx.fetch_modseq)
        .await
        .map_err(imap_error_to_gateway)?;
    // The highest UID across the WHOLE current mailbox (committed prefix +
    // pending tail) is the finalized watermark; ascending order guarantees it
    // lands in the last pending chunk (or, if the resume already caught up, is
    // the prior committed max).
    let overall_highest = all_uids.iter().map(|uid| uid.get()).max().map(ImapUid);
    let pending = uids_above(&all_uids, after_uid);
    let chunk_count = pending.len().div_ceil(INITIAL_SNAPSHOT_CHUNK_SIZE);

    let mut committed = 0usize;
    let mut highest_modseq_seen: Option<ImapModSeq> = None;
    for (chunk_index, chunk) in pending.chunks(INITIAL_SNAPSHOT_CHUNK_SIZE).enumerate() {
        let headers = fetch_header_chunk(
            client,
            &selected,
            chunk,
            ctx.fetch_modseq,
            ctx.fetch_gmail_metadata,
            ctx.updated_at,
        )
        .await
        .map_err(imap_error_to_gateway)?;
        for header in &headers {
            if let Some(modseq) = header.location.modseq {
                highest_modseq_seen =
                    Some(highest_modseq_seen.map_or(modseq, |current| current.max(modseq)));
            }
        }
        committed += headers.len();
        let is_last = chunk_index + 1 == chunk_count;
        let state = if is_last {
            finalized_state(&selected, overall_highest, highest_modseq_seen, ctx.updated_at)
        } else {
            // Highest UID committed so far. Ascending chunks make this chunk's
            // max the running max; a restart resumes strictly above it.
            let checkpoint = chunk.iter().map(|uid| uid.get()).max().map(ImapUid);
            partial_state(&selected, checkpoint, ctx.updated_at)
        };
        // Upsert-only checkpoint commit (rows + cursor) in one store transaction.
        sink.emit(imap_initial_snapshot_chunk_batch(discovery, headers, state))
            .await?;
        report_sync_progress(
            ctx.progress,
            ImapSyncProgressUpdate::new(SyncProgressStage::Fetching, "Fetching mailbox")
                .with_mailbox(mailbox.name.clone(), mailbox_ordinal, mailbox_count)
                .with_message_count(committed),
        );
    }

    if pending.is_empty() {
        // Nothing new to fetch (empty mailbox, or a resume that already caught
        // up): still emit a finalizing chunk so the mailbox leaves the
        // partial-initial-sync state and can take the delta path next cycle.
        sink.emit(imap_initial_snapshot_chunk_batch(
            discovery,
            Vec::new(),
            finalized_state(&selected, overall_highest, highest_modseq_seen, ctx.updated_at),
        ))
        .await?;
    }

    ph_info!(
        events::IMAP_MAILBOX_HEADER_FETCH_COMPLETED,
        account_id = %ctx.account_id,
        mailbox_id = %mailbox.id,
        mailbox_index = mailbox_ordinal,
        mailbox_count,
        mode = if resuming { "resume_initial_snapshot" } else { "initial_snapshot" },
        message_count = committed,
        duration_ms = started.elapsed().as_millis() as u64,
        "IMAP mailbox header fetch completed"
    );
    Ok(())
}

/// Checkpoint state for a non-final chunk: `highest_uid`/`highest_modseq` stay
/// `None` (the snapshot is incomplete) and `partial_initial_uid` records the
/// durable resume point.
fn partial_state(
    selected: &ImapSelectedMailbox,
    checkpoint: Option<ImapUid>,
    updated_at: &str,
) -> ImapMailboxSyncState {
    ImapMailboxSyncState {
        mailbox_id: selected.mailbox_id.clone(),
        mailbox_name: selected.mailbox_name.clone(),
        uid_validity: selected.uid_validity,
        highest_uid: None,
        highest_modseq: None,
        partial_initial_uid: checkpoint,
        updated_at: updated_at.to_string(),
    }
}

/// Completed state: the snapshot is whole, so `partial_initial_uid` is cleared
/// and the authoritative watermarks are written. The EXAMINE `[HIGHESTMODSEQ]`
/// is preferred over the max of fetched per-message MODSEQs (a resumed sync only
/// sees the tail's MODSEQs), matching `imap_mailbox_state_from_header_snapshot`.
fn finalized_state(
    selected: &ImapSelectedMailbox,
    highest_uid: Option<ImapUid>,
    highest_modseq_seen: Option<ImapModSeq>,
    updated_at: &str,
) -> ImapMailboxSyncState {
    ImapMailboxSyncState {
        mailbox_id: selected.mailbox_id.clone(),
        mailbox_name: selected.mailbox_name.clone(),
        uid_validity: selected.uid_validity,
        highest_uid,
        highest_modseq: selected.highest_modseq.or(highest_modseq_seen),
        partial_initial_uid: None,
        updated_at: updated_at.to_string(),
    }
}
