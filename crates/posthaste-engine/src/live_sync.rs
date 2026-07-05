use std::sync::Arc;
use std::time::Instant;

use jmap_client::client::Client;
use posthaste_domain_model::{
    AccountId, GatewayError, SyncBatch, SyncCursor, SyncObject, SyncOutcome, SyncProgress,
    SyncProgressStage, SyncReconciliation, SyncTrigger,
};
use posthaste_domain_service::{SyncChunkSink, SyncProgressReporter};
use posthaste_observability::{events, ph_info};

use crate::sync::{
    fetch_email_sync, fetch_email_sync_streamed, fetch_mailbox_sync, StreamedEmailSync,
};

/// Perform a full sync cycle: mailbox state then email state.
///
/// Falls back from delta to full sync on `cannotCalculateChanges`.
///
/// @spec docs/L1-sync#sync-loop
/// @spec docs/L1-sync#state-management
pub(crate) async fn sync_account(
    client: &Arc<Client>,
    cursors: &[SyncCursor],
    progress: Option<SyncProgressReporter>,
) -> Result<SyncBatch, GatewayError> {
    let mailbox_cursor = cursors
        .iter()
        .find(|cursor| cursor.object_type == SyncObject::Mailbox)
        .map(|cursor| cursor.state.as_str());
    let message_cursor = cursors
        .iter()
        .find(|cursor| cursor.object_type == SyncObject::Message)
        .map(|cursor| cursor.state.as_str());

    ph_info!(
        events::JMAP_SYNC_FETCH_STARTED,
        has_mailbox_state = mailbox_cursor.is_some(),
        has_message_state = message_cursor.is_some(),
        "JMAP sync fetch started"
    );
    report_progress(
        &progress,
        JmapSyncProgressUpdate::new(SyncProgressStage::Discovering, "Checking JMAP state"),
    );
    let mailbox_start = Instant::now();
    report_progress(
        &progress,
        JmapSyncProgressUpdate::new(SyncProgressStage::Fetching, "Fetching mailboxes"),
    );
    let mailbox_sync = fetch_mailbox_sync(client, mailbox_cursor).await?;
    ph_info!(
        events::JMAP_SYNC_MAILBOX_FETCHED,
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
    report_progress(
        &progress,
        JmapSyncProgressUpdate::new(SyncProgressStage::Fetching, "Fetching messages"),
    );
    let email_sync = fetch_email_sync(client, message_cursor).await?;
    ph_info!(
        events::JMAP_SYNC_BATCH_FETCHED,
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
        deleted_imap_message_locations: Vec::new(),
        deleted_mailbox_ids: mailbox_sync.deleted_mailbox_ids,
        deleted_message_ids: email_sync.deleted_message_ids,
        absence_deleted_imap_message_locations: Vec::new(),
        absence_deleted_message_ids: Vec::new(),
        replace_all_mailboxes: mailbox_sync.replace_all_mailboxes,
        replace_all_messages: email_sync.replace_all_messages,
        cursors: vec![mailbox_sync.cursor, email_sync.cursor],
    })
}

/// Streaming counterpart to [`sync_account`]: fetches mailbox state, emits it as
/// an upsert-only chunk, then streams email metadata pages as chunks so mail
/// surfaces progressively. A delta email sync is emitted as one self-reconciling
/// chunk carrying its own removals and cursors; a full snapshot streams
/// upsert-only pages and returns a [`SyncReconciliation`] so the service prunes
/// locals absent from the complete remote set and commits the withheld cursors.
///
/// @spec docs/L1-sync#sync-loop
pub(crate) async fn sync_account_streamed(
    client: &Arc<Client>,
    account_id: &AccountId,
    cursors: &[SyncCursor],
    progress: Option<SyncProgressReporter>,
    sink: &mut dyn SyncChunkSink,
) -> Result<SyncOutcome, GatewayError> {
    let _ = account_id;
    let mailbox_cursor = cursors
        .iter()
        .find(|cursor| cursor.object_type == SyncObject::Mailbox)
        .map(|cursor| cursor.state.as_str());
    let message_cursor = cursors
        .iter()
        .find(|cursor| cursor.object_type == SyncObject::Message)
        .map(|cursor| cursor.state.as_str());

    report_progress(
        &progress,
        JmapSyncProgressUpdate::new(SyncProgressStage::Discovering, "Checking JMAP state"),
    );
    report_progress(
        &progress,
        JmapSyncProgressUpdate::new(SyncProgressStage::Fetching, "Fetching mailboxes"),
    );
    let mailbox_sync = fetch_mailbox_sync(client, mailbox_cursor).await?;

    // Emit the mailbox chunk first so message rows never reference a mailbox
    // that has not been upserted yet. A full mailbox snapshot withholds its
    // pruning (and cursor) for the reconciliation pass; a delta carries its
    // explicit removals in this chunk.
    if !mailbox_sync.mailboxes.is_empty() || !mailbox_sync.deleted_mailbox_ids.is_empty() {
        sink.emit(SyncBatch {
            mailboxes: mailbox_sync.mailboxes.clone(),
            deleted_mailbox_ids: if mailbox_sync.replace_all_mailboxes {
                Vec::new()
            } else {
                mailbox_sync.deleted_mailbox_ids.clone()
            },
            ..SyncBatch::default()
        })
        .await?;
    }

    report_progress(
        &progress,
        JmapSyncProgressUpdate::new(SyncProgressStage::Fetching, "Fetching messages"),
    );
    // `emit` is `async` (D63/M23b): the sink is threaded straight through to
    // `fetch_email_sync_streamed`/`fetch_email_full_streamed` (rather than via
    // a synchronous `FnMut` callback), which `.await` it per page.
    let email = fetch_email_sync_streamed(client, message_cursor, sink).await?;

    // Reconciliation is needed when either object type is a full snapshot:
    // pruning by difference against the complete remote set cannot be done
    // mid-stream. When both are deltas, the message chunk carries the removals
    // and cursors and the stream self-reconciles.
    let prune_mailboxes = mailbox_sync.replace_all_mailboxes;
    match email {
        StreamedEmailSync::Delta(message_sync) => {
            if prune_mailboxes {
                // Mailbox full snapshot + message delta: emit the delta chunk's
                // removals now, defer mailbox pruning + cursors to the pass.
                sink.emit(SyncBatch {
                    messages: message_sync.messages,
                    deleted_message_ids: message_sync.deleted_message_ids,
                    ..SyncBatch::default()
                })
                .await?;
                Ok(SyncOutcome {
                    reconciliation: Some(SyncReconciliation {
                        remote_mailbox_ids: mailbox_sync
                            .mailboxes
                            .iter()
                            .map(|mailbox| mailbox.id.clone())
                            .collect(),
                        prune_mailboxes: true,
                        cursors: vec![mailbox_sync.cursor, message_sync.cursor],
                        ..SyncReconciliation::default()
                    }),
                })
            } else {
                // Both deltas: one self-reconciling chunk carries removals and
                // both cursors.
                sink.emit(SyncBatch {
                    messages: message_sync.messages,
                    deleted_message_ids: message_sync.deleted_message_ids,
                    cursors: vec![mailbox_sync.cursor, message_sync.cursor],
                    ..SyncBatch::default()
                })
                .await?;
                Ok(SyncOutcome::single_batch())
            }
        }
        StreamedEmailSync::FullStreamed {
            remote_message_ids,
            remote_ids_complete,
            cursor: message_cursor,
        } => Ok(SyncOutcome {
            reconciliation: Some(SyncReconciliation {
                remote_message_ids,
                remote_mailbox_ids: if prune_mailboxes {
                    mailbox_sync
                        .mailboxes
                        .iter()
                        .map(|mailbox| mailbox.id.clone())
                        .collect()
                } else {
                    Vec::new()
                },
                // DS1 mail-loss guard: only prune-by-absence when the remote id
                // set was proven exhaustive. A capped/incomplete `Email/query`
                // still upserts every page it retrieved (already emitted to the
                // sink above), but must NOT drive deletion, or mail beyond the
                // server cap would be durably removed from the local store.
                prune_messages: remote_ids_complete,
                prune_mailboxes,
                cursors: vec![mailbox_sync.cursor, message_cursor],
            }),
        }),
    }
}

struct JmapSyncProgressUpdate {
    stage: SyncProgressStage,
    detail: &'static str,
}

impl JmapSyncProgressUpdate {
    fn new(stage: SyncProgressStage, detail: &'static str) -> Self {
        Self { stage, detail }
    }
}

fn report_progress(reporter: &Option<SyncProgressReporter>, update: JmapSyncProgressUpdate) {
    if let Some(reporter) = reporter {
        reporter.report(SyncProgress {
            sync_id: String::new(),
            trigger: SyncTrigger::Manual,
            started_at: String::new(),
            stage: update.stage,
            detail: update.detail.to_string(),
            mailbox_name: None,
            mailbox_index: None,
            mailbox_count: None,
            message_count: None,
            total_count: None,
        });
    }
}
