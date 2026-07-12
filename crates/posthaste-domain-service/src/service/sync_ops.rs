use std::sync::Arc;

use async_trait::async_trait;

use super::*;
use crate::{SyncChunkSink, SyncWriteStore};
use posthaste_domain_model::{MessageRecord, SyncBatch};

/// Applies and publishes each sync chunk as the gateway emits it, accumulating
/// the per-chunk messages and counts the post-sync steps need. Chunk events are
/// published immediately so mail surfaces progressively.
///
/// `sync_writer` is an owned `Arc` (not a borrow): `emit` moves a clone of it
/// onto the tokio blocking pool via [`offload`] (D63/M23b) — the store write
/// it drives is the heaviest work on the sync path (a snapshot/delta apply),
/// and [`SyncWriteStore`] stays a plain sync `&self` port (so
/// `posthaste-store`'s own unit tests keep calling it directly, no runtime
/// needed — see the port's doc comment), so the offload happens here at the
/// call site instead. `Arc::clone` is a cheap refcount bump, paid once per
/// chunk emit.
struct ServiceSyncSink<'a> {
    sync_writer: Arc<dyn SyncWriteStore>,
    account_id: &'a AccountId,
    publish: &'a mut (dyn FnMut(&[DomainEvent]) + Send),
    applied_events: Vec<DomainEvent>,
    messages: Vec<MessageRecord>,
    mailbox_count: usize,
    deleted_imap_location_count: usize,
    deleted_message_count: usize,
    error: Option<ServiceError>,
}

// NS1 cutover note: the M35 `guard_unsettled` sync-time fold and the
// `protected_message_ids` prune exemption are GONE. Sync writes RAW provider
// truth to base; un-acked optimism lives only in the overlay plane
// (`message_overlay`), which every effective read folds in and which the
// post-sync sweep below re-derives over the fresh base. A not-yet-settled
// local effect therefore survives any snapshot without base ever holding
// folded state: a pending flag rides the overlay; a pending Destroy is an
// overlay tombstone the snapshot upsert cannot resurrect; a server
// delete-delta for an overlaid message removes only the base row while the
// overlay keeps serving the pending intent until its op settles.

#[async_trait]
impl SyncChunkSink for ServiceSyncSink<'_> {
    async fn emit(&mut self, mut batch: SyncBatch) -> Result<(), GatewayError> {
        // Offloaded (D63/M23b): `batch` is cloned once into the 'static
        // closure spawn_blocking requires; `batch` itself stays owned here for
        // the post-apply counts/append below.
        let sync_writer = self.sync_writer.clone();
        let account_id = self.account_id.clone();
        let owned_batch = batch.clone();
        let result = offload(move || {
            sync_writer.apply_sync_batch(&BaseWrite::reconciler(), &account_id, &owned_batch)
        })
        .await;
        match result {
            Ok(events) => {
                (self.publish)(&events);
                self.applied_events.extend(events);
                self.mailbox_count += batch.mailboxes.len();
                self.deleted_imap_location_count += batch.deleted_imap_message_locations.len();
                self.deleted_message_count += batch.deleted_message_ids.len();
                self.messages.append(&mut batch.messages);
                Ok(())
            }
            Err(error) => {
                self.error = Some(error.into());
                Err(GatewayError::Rejected(
                    "sync chunk could not be applied".to_string(),
                ))
            }
        }
    }
}

impl MailService {
    /// Run a full sync cycle: load cursors, fetch delta, apply batch, emit events.
    ///
    /// @spec docs/L1-sync#sync-loop
    pub async fn sync_account(
        &self,
        account_id: &AccountId,
        trigger: SyncTrigger,
        gateway: &dyn MailGateway,
        progress: Option<crate::SyncProgressReporter>,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        // No live publisher: the caller publishes the returned events.
        let mut publish = |_: &[DomainEvent]| {};
        self.sync_account_with_mode(
            account_id,
            trigger,
            SyncMode::Incremental,
            gateway,
            progress,
            &mut publish,
        )
        .await
    }

    /// Run a sync cycle with an explicit user-requested mode.
    ///
    /// `publish` receives each event group as it is produced (per applied
    /// chunk, then automation/settlement/completion), so a streaming caller can
    /// broadcast mail as it arrives instead of after the whole sync. The full
    /// event set is still returned for callers that publish at the end.
    ///
    /// @spec docs/L1-sync#sync-loop
    pub async fn sync_account_with_mode(
        &self,
        account_id: &AccountId,
        trigger: SyncTrigger,
        mode: SyncMode,
        gateway: &dyn MailGateway,
        progress: Option<crate::SyncProgressReporter>,
        publish: &mut (dyn FnMut(&[DomainEvent]) + Send),
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let mut cursors = self.sync_state.get_sync_cursors(account_id)?;
        if mode.requires_full_message_metadata() {
            cursors.retain(|cursor| cursor.object_type != SyncObject::Message);
        }
        // FLUSH: push pending local-first ops before the pull so the observe
        // sees post-mutation provider state. Successful message state
        // assertions rest in `applied` (folded by the read overlay); they are
        // retired below once this sync writes their effect into the projection.
        // Best-effort: offline leaves them pending and the overlay still folds.
        //
        // @spec docs/replication/L1#convergence-cycle
        let mut events = Vec::new();
        let flush_events = self.flush_account(account_id, gateway).await?;
        publish(&flush_events);
        events.extend(flush_events);

        // OBSERVE: stream the pull as chunks, applying + publishing each so mail
        // surfaces progressively. The sink accumulates messages/counts; the
        // outcome carries any reconciliation set for the final pass. Base
        // receives RAW provider truth (NS1) — the overlay sweep below refolds
        // any still-unsettled optimism over the fresh base afterwards.
        let (
            sync_messages,
            mailbox_count,
            deleted_imap_location_count,
            deleted_message_count,
            outcome,
        ) = {
            let mut sink = ServiceSyncSink {
                sync_writer: self.sync_writer.clone(),
                account_id,
                publish,
                applied_events: Vec::new(),
                messages: Vec::new(),
                mailbox_count: 0,
                deleted_imap_location_count: 0,
                deleted_message_count: 0,
                error: None,
            };
            let result = gateway
                .sync_streamed(account_id, &cursors, progress.clone(), &mut sink)
                .await;
            let outcome = match result {
                Ok(outcome) => outcome,
                Err(error) => {
                    // Surface the underlying apply failure when the stream
                    // aborted because a chunk could not be written.
                    return Err(sink.error.take().unwrap_or_else(|| error.into()));
                }
            };
            events.append(&mut sink.applied_events);
            (
                std::mem::take(&mut sink.messages),
                sink.mailbox_count,
                sink.deleted_imap_location_count,
                sink.deleted_message_count,
                outcome,
            )
        };

        // RECONCILE (final pass): when the gateway streamed upsert-only chunks,
        // prune locals absent from the complete remote set and commit the
        // withheld cursors in one transaction. A single self-reconciling batch
        // (delta syncs, the default gateway path) leaves this `None`, having
        // carried its own removals and cursors in the chunk.
        //
        if let Some(reconciliation) = &outcome.reconciliation {
            let sync_writer = self.sync_writer.clone();
            let owned_account_id = account_id.clone();
            let owned_reconciliation = reconciliation.clone();
            let reconcile_events = offload(move || {
                sync_writer.reconcile_sync(
                    &BaseWrite::reconciler(),
                    &owned_account_id,
                    &owned_reconciliation,
                )
            })
            .await?;
            publish(&reconcile_events);
            events.extend(reconcile_events);
        }
        if let Some(progress) = progress {
            progress.report(posthaste_domain_model::SyncProgress {
                sync_id: String::new(),
                trigger: trigger.clone(),
                started_at: String::new(),
                stage: posthaste_domain_model::SyncProgressStage::Storing,
                detail: "Applying synced changes".to_string(),
                mailbox_name: None,
                mailbox_index: None,
                mailbox_count: None,
                message_count: Some(sync_messages.len()),
                total_count: None,
            });
        }
        let mut post_commit_errors = Vec::new();
        if let Some(account) = self.config.get_source(account_id)? {
            let settings = self.config.get_app_settings()?;
            if let Err(error) = self.upsert_body_cache_candidates(
                account_id,
                &account,
                &settings.cache_policy,
                &sync_messages,
            ) {
                ph_warn!(
                    events::DOMAIN_CACHE_CANDIDATE_POST_SYNC_FAILED,
                    account_id = %account_id,
                    error = %error,
                    "post-sync body cache candidate update failed after sync batch commit"
                );
                post_commit_errors.push(error.code().to_string());
            }
        }
        let action_events = match self
            .apply_automation_rules(account_id, &sync_messages, gateway)
            .await
        {
            Ok(events) => events,
            Err(error) => {
                ph_warn!(
                    events::DOMAIN_AUTOMATION_POST_SYNC_FAILED,
                    account_id = %account_id,
                    error = %error,
                    "post-sync automation failed after sync batch commit"
                );
                post_commit_errors.push(error.code().to_string());
                Vec::new()
            }
        };
        let action_count = action_events.len();
        publish(&action_events);
        events.extend(action_events);
        // Push any ops automation enqueued while applying the batch; they rest
        // in `applied` and retire on the next sync cycle.
        match self.flush_account(account_id, gateway).await {
            Ok(settlement_events) => {
                publish(&settlement_events);
                events.extend(settlement_events);
            }
            Err(error) => {
                ph_warn!(
                    events::DOMAIN_AUTOMATION_POST_SYNC_FAILED,
                    account_id = %account_id,
                    error = %error,
                    "post-sync automation outbox flush failed after sync batch commit"
                );
                post_commit_errors.push(error.code().to_string());
            }
        }
        // NS1 overlay sweep: refold every still-overlaid message over the base
        // this sync just rewrote (or drop entries whose ops settled during the
        // flush legs). The inventory is bounded by the pending outbox — small.
        let sweep_events = self.sweep_message_overlay(account_id).await?;
        publish(&sweep_events);
        events.extend(sweep_events);
        let sync_event = self.events.append_event(
        account_id,
        EVENT_TOPIC_SYNC_COMPLETED,
        None,
        None,
        json!({
            "mailboxCount": mailbox_count,
            "messageCount": sync_messages.len(),
            "deletedImapLocationCount": deleted_imap_location_count,
            "deletedMessageCount": deleted_message_count,
            "automationEventCount": action_count,
            "trigger": trigger.as_str(),
            "mode": mode.as_str(),
            "resources": [
                { "kind": "sync", "operation": "completed", "accountId": account_id.as_str(), "mode": mode.as_str() },
                { "kind": "mailbox", "operation": "refreshed", "accountId": account_id.as_str() },
                { "kind": "message", "operation": "refreshed", "accountId": account_id.as_str() },
            ],
            "postCommitErrors": post_commit_errors,
        }),
    )?;
        publish(std::slice::from_ref(&sync_event));
        events.push(sync_event);
        Ok(events)
    }

    /// One convergence observation: flush pending ops, pull authoritative
    /// provider state into the projection, and retire the message assertions
    /// the pull confirmed. Used where a projection-walking loop (automation
    /// backfill) must see its own progress reflected in the projection before
    /// the next batch query, since query filters read the projection before the
    /// read overlay folds.
    ///
    /// @spec docs/replication/L1#convergence-cycle
    pub(crate) async fn flush_and_observe(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let mut events = self.flush_account(account_id, gateway).await?;
        let cursors = self.sync_state.get_sync_cursors(account_id)?;
        let batch = gateway.sync(account_id, &cursors, None).await?;
        // NS1: raw provider truth to base; the sweep refolds any surviving
        // optimism over it afterwards.
        let sync_writer = self.sync_writer.clone();
        let owned_account_id = account_id.clone();
        let owned_batch = batch.clone();
        events.extend(
            offload(move || {
                sync_writer.apply_sync_batch(
                    &BaseWrite::reconciler(),
                    &owned_account_id,
                    &owned_batch,
                )
            })
            .await?,
        );
        events.extend(self.sweep_message_overlay(account_id).await?);
        Ok(events)
    }

    /// Re-derive every overlaid message for the account (NS1): the sync-side
    /// leg of the overlay lifecycle. Entries whose ops settled are removed;
    /// entries with surviving ops are refolded over the just-written base.
    /// Returns the adoption prune echoes (NS2 Slice 4) for the caller to
    /// publish.
    async fn sweep_message_overlay(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let overlay_ids = {
            let overlay = self.overlay.clone();
            let owned_account_id = account_id.clone();
            offload(move || overlay.list_overlay_message_ids(&owned_account_id)).await?
        };
        let mut events = Vec::new();
        for message_id in overlay_ids {
            // NS2 Slice 4 adoption: a provisional Sent row whose provider
            // copy landed in base (matched by the transport-shared
            // Message-ID prefix) retires WITH a prune echo — the visible row
            // changes identity, so the client must drop the provisional id.
            if let Some(event) = self
                .try_adopt_provisional_sent(account_id, &message_id)
                .await?
            {
                events.push(event);
                continue;
            }
            // Retire-on-confirmation: an all-settled entry is removed only
            // once this sync's base write actually carries its effect.
            self.refresh_message_overlay(
                account_id,
                &message_id,
                super::mutation::OverlayRetire::ConfirmAgainstBase,
            )
            .await?;
        }
        Ok(events)
    }

    /// Retire a provisional Sent overlay row whose provider copy has arrived
    /// in base under its own id (NS2 Slice 4, reconcile-by-intent-id +
    /// adopt-by-header). `None` = not such a row / not adopted yet.
    async fn try_adopt_provisional_sent(
        &self,
        account_id: &AccountId,
        row_id: &MessageId,
    ) -> Result<Option<DomainEvent>, ServiceError> {
        let entry = {
            let overlay = self.overlay.clone();
            let owned_account = account_id.clone();
            let owned_row = row_id.clone();
            offload(move || overlay.read_overlay_message(&owned_account, &owned_row)).await?
        };
        let Some(Some(folded)) = entry else {
            return Ok(None);
        };
        // Only send-minted rows: the transport-shared identity token, and
        // never a draft row (those retire via base coverage).
        let Some(prefix) = folded
            .rfc_message_id
            .as_deref()
            .filter(|rfc| rfc.starts_with("phsend-"))
            .and_then(|rfc| rfc.split_once('@'))
            .map(|(token, _)| format!("{token}@"))
        else {
            return Ok(None);
        };
        if folded.draft_id.is_some() {
            return Ok(None);
        }
        // A live op still keyed here (pending/held/parked send) owns the
        // entry — adoption applies only to settled rows awaiting the sync.
        let has_live_op = self
            .outbox
            .list_unsettled_operations(account_id)?
            .into_iter()
            .any(|op| op.entity.id == row_id.as_str());
        if has_live_op {
            return Ok(None);
        }
        let adopted = {
            let overlay = self.overlay.clone();
            let owned_account = account_id.clone();
            offload(move || overlay.find_base_message_id_by_rfc_prefix(&owned_account, &prefix))
                .await?
        };
        if adopted.is_none() {
            return Ok(None);
        }
        {
            let overlay = self.overlay.clone();
            let owned_account = account_id.clone();
            let owned_row = row_id.clone();
            offload(move || overlay.remove_overlay_message(&owned_account, &owned_row)).await?;
        }
        Ok(Some(self.events.append_event(
            account_id,
            EVENT_TOPIC_MESSAGE_UPDATED,
            None,
            Some(row_id),
            json!({ "messageId": row_id.as_str(), "deleted": true }),
        )?))
    }

    /// Append a `sync.failed` event to the event log.
    ///
    /// @spec docs/L1-sync#error-handling
    pub fn record_sync_failure(
        &self,
        account_id: &AccountId,
        code: &str,
        message: &str,
        trigger: SyncTrigger,
        stage: &str,
    ) -> Result<DomainEvent, ServiceError> {
        self.events
        .append_event(
            account_id,
            EVENT_TOPIC_SYNC_FAILED,
            None,
            None,
            json!({
                "code": code,
                "message": message,
                "trigger": trigger.as_str(),
                "stage": stage,
                "resources": [
                    { "kind": "sync", "operation": "failed", "accountId": account_id.as_str() },
                    { "kind": "accountRuntime", "operation": "updated", "accountId": account_id.as_str() },
                ],
            }),
        )
        .map_err(Into::into)
    }
}
