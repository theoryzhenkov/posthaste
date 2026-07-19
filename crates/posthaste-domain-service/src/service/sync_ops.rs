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
    /// Drives the per-base-write replay: each applied chunk re-derives the
    /// override rows it touched before its events are published.
    service: &'a MailService,
    account_id: &'a AccountId,
    publish: &'a mut (dyn FnMut(&[DomainEvent]) + Send),
    applied_events: Vec<DomainEvent>,
    messages: Vec<MessageRecord>,
    mailbox_count: usize,
    deleted_imap_location_count: usize,
    deleted_message_count: usize,
    error: Option<ServiceError>,
}

// Sync writes RAW provider truth to base; local optimism lives only in the
// overlay plane (`message_overlay`), which every effective read folds in and
// which the per-chunk replay plus the post-sync sweep re-derive over the
// fresh base. A local effect not yet absorbed by the sync chain — pending,
// inflight, or settled-awaiting-truncation — therefore survives any snapshot
// without base ever holding folded state: a pending flag rides the overlay;
// a pending Destroy is an overlay tombstone the snapshot upsert cannot
// resurrect; a server delete-delta for an overlaid message removes the base
// row AND the replay drops the pending intent's fold with it (base wins —
// only a pending Destroy keeps its tombstone).

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
                // This chunk is a base write: re-derive the override rows it
                // touched BEFORE publishing, so no effective read (or
                // event-driven re-query) observes a pending op's fold over
                // the old base — and an abort later in the cycle leaves no
                // stale override behind.
                let mut written: std::collections::BTreeSet<MessageId> = batch
                    .messages
                    .iter()
                    .map(|record| record.id.clone())
                    .collect();
                written.extend(batch.deleted_message_ids.iter().cloned());
                written.extend(batch.absence_deleted_message_ids.iter().cloned());
                written.extend(
                    batch
                        .deleted_imap_message_locations
                        .iter()
                        .chain(&batch.absence_deleted_imap_message_locations)
                        .map(|location| location.message_id.clone()),
                );
                if let Err(error) = self
                    .service
                    .replay_base_write(self.account_id, &written)
                    .await
                {
                    self.error = Some(error);
                    return Err(GatewayError::Rejected(
                        "post-chunk overlay replay failed".to_string(),
                    ));
                }
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
        // Stamped at cycle ENTRY (before the flush leg) on the same
        // monotonic-anchored clock that stamps settlement markers: an op this
        // cycle's own flush settles is NOT "settled before a cycle that
        // started after it", so it bridges until the NEXT completed cycle.
        // The sweep's truncation pass reads this as its cycle clock.
        let cycle_started_mono = super::outbox::schedule::monotonic_now_secs();
        let mut cursors = self.sync_state.get_sync_cursors(account_id)?;
        if mode.requires_full_message_metadata() {
            cursors.retain(|cursor| cursor.object_type != SyncObject::Message);
        }
        // FLUSH: push pending local-first ops before the pull so the observe
        // sees post-mutation provider state. Blind settlements (no provider
        // readback) rest in `applied` — still folded by the read overlay —
        // until a later cycle's causal truncation. Best-effort: offline
        // leaves ops pending and the overlay still folds.
        //
        // @spec docs/replication/L1#convergence-cycle
        let mut events = Vec::new();
        let flush_events = self.flush_account(account_id, gateway).await?;
        publish(&flush_events);
        events.extend(flush_events);

        // OBSERVE: stream the pull as chunks, applying + publishing each so mail
        // surfaces progressively. The sink accumulates messages/counts; the
        // outcome carries any reconciliation set for the final pass. Base
        // receives RAW provider truth; each applied chunk re-derives the
        // override rows it touched, and the end-of-cycle sweep below refolds
        // whatever the flush legs settled.
        let (
            sync_messages,
            mailbox_count,
            deleted_imap_location_count,
            deleted_message_count,
            outcome,
        ) = {
            let mut sink = ServiceSyncSink {
                sync_writer: self.sync_writer.clone(),
                service: self,
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
            // The prune is a base write too: re-derive the pruned rows'
            // overrides before publishing.
            let pruned: std::collections::BTreeSet<MessageId> = reconcile_events
                .iter()
                .filter_map(|event| event.message_id.clone())
                .collect();
            self.replay_base_write(account_id, &pruned).await?;
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
        // Push any ops automation enqueued while applying the batch; blind
        // settlements rest in `applied` and truncate on a later cycle.
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
        // Replay on the base this sync just rewrote: truncate settled ops the
        // chain has absorbed, then re-derive every row in the replay
        // inventory (op-touched ∪ overlaid). Bounded by the outbox — small.
        let sweep_events = self
            .sweep_message_overlay(account_id, cycle_started_mono)
            .await?;
        publish(&sweep_events);
        events.extend(sweep_events);
        let sync_event = self.events.append_event(
            account_id,
            EVENT_TOPIC_SYNC_COMPLETED,
            None,
            None,
            // Typed contract: `posthaste_domain_model::SyncCompletedPayload`
            // (mirrored into the client protocol).
            serde_json::to_value(posthaste_domain_model::SyncCompletedPayload {
                mailbox_count,
                message_count: sync_messages.len(),
                deleted_imap_location_count,
                deleted_message_count,
                automation_event_count: action_count,
                trigger,
                mode,
                resources: vec![
                    posthaste_domain_model::SyncResourceRef {
                        kind: "sync".to_string(),
                        operation: "completed".to_string(),
                        account_id: account_id.clone(),
                        mode: Some(mode),
                    },
                    posthaste_domain_model::SyncResourceRef {
                        kind: "mailbox".to_string(),
                        operation: "refreshed".to_string(),
                        account_id: account_id.clone(),
                        mode: None,
                    },
                    posthaste_domain_model::SyncResourceRef {
                        kind: "message".to_string(),
                        operation: "refreshed".to_string(),
                        account_id: account_id.clone(),
                        mode: None,
                    },
                ],
                post_commit_errors,
            })
            .unwrap_or(serde_json::Value::Null),
        )?;
        publish(std::slice::from_ref(&sync_event));
        events.push(sync_event);
        Ok(events)
    }

    /// One convergence observation: flush pending ops, pull authoritative
    /// provider state into the projection, and run the sweep (truncation +
    /// replay). Used where a projection-walking loop (automation backfill)
    /// must see its own progress reflected in the projection before the next
    /// batch query, since query filters read the projection before the read
    /// overlay folds.
    ///
    /// @spec docs/replication/L1#convergence-cycle
    pub(crate) async fn flush_and_observe(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        // Cycle-entry stamp, same discipline as `sync_account_with_mode`: ops
        // settled by THIS flush wait for the next completed cycle.
        let cycle_started_mono = super::outbox::schedule::monotonic_now_secs();
        let mut events = self.flush_account(account_id, gateway).await?;
        let cursors = self.sync_state.get_sync_cursors(account_id)?;
        let batch = gateway.sync(account_id, &cursors, None).await?;
        // Raw provider truth to base; the sweep refolds any surviving
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
        events.extend(
            self.sweep_message_overlay(account_id, cycle_started_mono)
                .await?,
        );
        Ok(events)
    }

    /// The completed-cycle sweep: causal truncation first, then re-derive
    /// every row in the replay inventory — rows the replayable log touches
    /// (so a base row rewritten by this sync under a pending op re-derives
    /// even if its override row was never written or was wiped) plus rows
    /// currently overlaid. The per-chunk `replay_base_write` covers rows as
    /// each base write lands; this sweep adds the whole-cycle passes —
    /// truncation of settled ops the sync chain has absorbed,
    /// provisional-Sent adoption, and tombstone repair. Runs only after a
    /// cycle completes (an aborted stream returns before it), so truncation
    /// never fires on an incomplete pull. Returns the adoption prune echoes
    /// for the caller to publish.
    async fn sweep_message_overlay(
        &self,
        account_id: &AccountId,
        cycle_started_mono: i64,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        self.truncate_settled_operations(account_id, cycle_started_mono)
            .await?;
        let inventory = self.replay_inventory(account_id).await?;
        let mut events = Vec::new();
        for message_id in inventory {
            // Adoption: a provisional Sent row whose provider
            // copy landed in base (matched by the transport-shared
            // Message-ID prefix) retires WITH a prune echo — the visible row
            // changes identity, so the client must drop the provisional id.
            if let Some(adoption_events) = self
                .try_adopt_provisional_sent(account_id, &message_id)
                .await?
            {
                events.extend(adoption_events);
                continue;
            }
            // D175 lingering-destruction repair: a TOMBSTONE whose base row
            // survived the sync this sweep follows means the provider still
            // holds what we committed to destroying (a lost IMAP expunge, a
            // silently no-opped JMAP destroy). One idempotent cleanup delete
            // re-asserts it — without this, the copy leaks behind a
            // permanent tombstone.
            if let Some(repair_events) = self
                .try_repair_lingering_tombstone(account_id, &message_id)
                .await?
            {
                events.extend(repair_events);
                continue;
            }
            // Re-derive from (log, base): entries whose ops truncated above
            // fall back to base; still-bridging settled ops keep folding.
            self.refresh_message_overlay(account_id, &message_id)
                .await?;
        }
        Ok(events)
    }

    /// D175: enqueue ONE idempotent provider delete for an orphaned tombstone
    /// (no outstanding op, base row still present after the sync). Bounded:
    /// any existing op keyed to the row — including a previous repair, even a
    /// failed one — blocks a re-enqueue; a premature repair is safe (the
    /// delete is `notFound`-masked). `None` = not such an entry.
    async fn try_repair_lingering_tombstone(
        &self,
        account_id: &AccountId,
        row_id: &MessageId,
    ) -> Result<Option<Vec<DomainEvent>>, ServiceError> {
        let entry = {
            let overlay = self.overlay.clone();
            let owned_account = account_id.clone();
            let owned_row = row_id.clone();
            offload(move || overlay.read_overlay_message(&owned_account, &owned_row)).await?
        };
        if !matches!(entry, Some(None)) {
            return Ok(None); // not a tombstone
        }
        let base_survives = {
            let overlay = self.overlay.clone();
            let owned_account = account_id.clone();
            let owned_row = row_id.clone();
            offload(move || overlay.read_base_message_record(&owned_account, &owned_row))
                .await?
                .is_some()
        };
        if !base_survives {
            return Ok(None); // sync pruned it — the ordinary retire handles this
        }
        // Any op keyed to the row, in ANY state, blocks the repair: a failed
        // prior repair (pending list) must not re-enqueue every cycle, and a
        // settled-awaiting-truncation Destroy (unsettled list) already
        // committed its provider delete — re-enqueueing during its bridge
        // window would issue a duplicate delete each cycle until truncation.
        let has_op = self
            .outbox
            .list_pending_operations(account_id)?
            .into_iter()
            .chain(self.outbox.list_unsettled_operations(account_id)?)
            .any(|op| op.entity.id == row_id.as_str());
        if has_op {
            return Ok(None); // its own lifecycle (or a prior repair) owns it
        }
        ph_warn!(
            events::OUTBOX_LINGERING_TOMBSTONE_REPAIRED,
            account_id = %account_id,
            message_id = %row_id,
            "destroyed message survived the sync in base; enqueueing one \
             idempotent cleanup delete (D175)"
        );
        let (_operation, delete_events) =
            self.delete_draft(account_id, row_id.clone(), true).await?;
        Ok(Some(delete_events))
    }

    /// Retire a provisional Sent overlay row whose provider copy has arrived
    /// in base under its own id (reconcile-by-intent-id + adopt-by-header),
    /// filing an unfiled copy into Sent (S-CONV-2).
    /// `None` = not such a row / not adopted yet.
    async fn try_adopt_provisional_sent(
        &self,
        account_id: &AccountId,
        row_id: &MessageId,
    ) -> Result<Option<Vec<DomainEvent>>, ServiceError> {
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
        // never a draft row (those leave once base absorbs their copy).
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
        let Some(adopted_id) = adopted else {
            return Ok(None);
        };
        {
            let overlay = self.overlay.clone();
            let owned_account = account_id.clone();
            let owned_row = row_id.clone();
            offload(move || overlay.remove_overlay_message(&owned_account, &owned_row)).await?;
        }
        let mut events = vec![self.events.append_event(
            account_id,
            EVENT_TOPIC_MESSAGE_UPDATED,
            None,
            Some(row_id),
            json!({ "messageId": row_id.as_str(), "deleted": true }),
        )?];
        // S-CONV-2: a delivered-but-UNFILED copy — the server ignored the
        // Drafts→Sent move / the Sent append failed, so the copy syncs back
        // AS A DRAFTS GHOST. File it with one ordinary mailbox assertion:
        // Sent is ADDED and only Drafts dropped (never a wholesale replace —
        // stripping other memberships would delete the copy on
        // all-mail-style providers). Triggered strictly on the ghost shape
        // (in Drafts, not in Sent): partially-applied mid-sync memberships
        // must never fire it.
        if let (Some(sent_mailbox), Some(drafts_mailbox)) = (
            self.mailbox_id_by_role(account_id, "sent")?,
            self.drafts_mailbox_id(account_id)?,
        ) {
            let adopted_record = {
                let overlay = self.overlay.clone();
                let owned_account = account_id.clone();
                let owned_adopted = adopted_id.clone();
                offload(move || overlay.read_base_message_record(&owned_account, &owned_adopted))
                    .await?
            };
            if let Some(record) = adopted_record {
                let is_drafts_ghost = record.mailbox_ids.contains(&drafts_mailbox)
                    && !record.mailbox_ids.contains(&sent_mailbox);
                if is_drafts_ghost {
                    ph_warn!(
                        events::SEND_ADOPTED_COPY_UNFILED,
                        account_id = %account_id,
                        message_id = %adopted_id,
                        "adopted sent copy is a Drafts ghost; filing it (S-CONV-2)"
                    );
                    let mut mailbox_ids: Vec<MailboxId> = record
                        .mailbox_ids
                        .iter()
                        .filter(|mailbox_id| **mailbox_id != drafts_mailbox)
                        .cloned()
                        .collect();
                    mailbox_ids.push(sent_mailbox);
                    let ack = self
                        .replace_mailboxes(
                            account_id,
                            &adopted_id,
                            &posthaste_domain_model::ReplaceMailboxesCommand { mailbox_ids },
                        )
                        .await?;
                    events.extend(ack.events);
                }
            }
        }
        Ok(Some(events))
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
