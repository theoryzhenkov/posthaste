use super::*;
use crate::{MessageRecord, SyncBatch, SyncChunkSink, SyncWriteStore};

/// Applies and publishes each sync chunk as the gateway emits it, accumulating
/// the per-chunk messages and counts the post-sync steps need. Chunk events are
/// published immediately so mail surfaces progressively.
///
/// @spec docs/stale/L1-sync#progressive-delivery-and-final-reconciliation
struct ServiceSyncSink<'a> {
    sync_writer: &'a dyn SyncWriteStore,
    account_id: &'a AccountId,
    /// Messages with an unsettled optimistic op: the sync must not overwrite
    /// their canonical row (S3 unsettled-guard). Reconciled at settle instead,
    /// from a fresh readback.
    unsettled: std::collections::HashSet<String>,
    publish: &'a mut (dyn FnMut(&[DomainEvent]) + Send),
    applied_events: Vec<DomainEvent>,
    messages: Vec<MessageRecord>,
    mailbox_count: usize,
    deleted_imap_location_count: usize,
    deleted_message_count: usize,
    error: Option<ServiceError>,
}

/// Drop messages with an unsettled optimistic op from a provider sync batch so
/// it cannot clobber an in-flight optimistic write (S3). A `replace_all`
/// snapshot is left unfiltered — dropping a message there would let the store
/// prune it; guarding the snapshot path needs store-internal support.
///
/// TODO(S3): replace_all snapshot does not yet guard unsettled messages.
fn guard_unsettled(batch: &mut SyncBatch, unsettled: &std::collections::HashSet<String>) {
    if unsettled.is_empty() || batch.replace_all_messages {
        return;
    }
    batch
        .messages
        .retain(|message| !unsettled.contains(message.id.as_str()));
    batch
        .deleted_message_ids
        .retain(|id| !unsettled.contains(id.as_str()));
}

impl SyncChunkSink for ServiceSyncSink<'_> {
    fn emit(&mut self, mut batch: SyncBatch) -> Result<(), GatewayError> {
        guard_unsettled(&mut batch, &self.unsettled);
        match self.sync_writer.apply_sync_batch(self.account_id, &batch) {
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
        // outcome carries any reconciliation set for the final pass.
        //
        // @spec docs/stale/L1-sync#progressive-delivery-and-final-reconciliation
        let (
            sync_messages,
            mailbox_count,
            deleted_imap_location_count,
            deleted_message_count,
            outcome,
        ) = {
            let mut sink = ServiceSyncSink {
                sync_writer: self.sync_writer.as_ref(),
                account_id,
                // Computed after FLUSH above, so just-settled ops (now removed)
                // are excluded and the sync applies their authoritative effect.
                unsettled: self.unsettled_message_ids(account_id)?,
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
        // @spec docs/stale/L1-sync#progressive-delivery-and-final-reconciliation
        if let Some(reconciliation) = &outcome.reconciliation {
            let reconcile_events = self
                .sync_writer
                .reconcile_sync(account_id, reconciliation)?;
            publish(&reconcile_events);
            events.extend(reconcile_events);
        }
        if let Some(progress) = progress {
            progress.report(crate::SyncProgress {
                sync_id: String::new(),
                trigger: trigger.clone(),
                started_at: String::new(),
                stage: crate::SyncProgressStage::Storing,
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
        let mut batch = gateway.sync(account_id, &cursors, None).await?;
        // S3 unsettled-guard: computed after FLUSH so just-settled ops are excluded.
        guard_unsettled(&mut batch, &self.unsettled_message_ids(account_id)?);
        events.extend(self.sync_writer.apply_sync_batch(account_id, &batch)?);
        Ok(events)
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

#[cfg(test)]
mod guard_tests {
    use super::guard_unsettled;
    use crate::{MessageId, MessageRecord, SyncBatch};
    use std::collections::HashSet;

    fn batch(upserts: &[&str], deletes: &[&str], replace_all_messages: bool) -> SyncBatch {
        SyncBatch {
            mailboxes: Vec::new(),
            messages: upserts
                .iter()
                .map(|id| MessageRecord {
                    id: MessageId::from(*id),
                    ..Default::default()
                })
                .collect(),
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            deleted_message_ids: deletes.iter().map(|id| MessageId::from(*id)).collect(),
            replace_all_mailboxes: false,
            replace_all_messages,
            cursors: Vec::new(),
        }
    }

    #[test]
    fn drops_unsettled_upserts_and_deletes_keeps_the_rest() {
        let unsettled = HashSet::from(["message-1".to_string()]);
        let mut b = batch(
            &["message-1", "message-2"],
            &["message-1", "message-3"],
            false,
        );
        guard_unsettled(&mut b, &unsettled);
        // message-1 (unsettled) is removed from both the upserts and the deletes,
        // so the provider's view cannot clobber its in-flight optimistic write.
        assert_eq!(
            b.messages.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["message-2"],
        );
        assert_eq!(
            b.deleted_message_ids
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>(),
            vec!["message-3"],
        );
    }

    #[test]
    fn keeps_everything_when_nothing_is_unsettled() {
        let unsettled = HashSet::new();
        let mut b = batch(&["message-1", "message-2"], &["message-1"], false);
        guard_unsettled(&mut b, &unsettled);
        assert_eq!(b.messages.len(), 2);
        assert_eq!(b.deleted_message_ids.len(), 1);
    }

    #[test]
    fn leaves_replace_all_snapshots_unfiltered() {
        // A replace_all snapshot is left untouched (dropping a message there would
        // let the store prune it) — TODO(S3) guards that path with store support.
        let unsettled = HashSet::from(["message-1".to_string()]);
        let mut b = batch(&["message-1"], &[], true);
        guard_unsettled(&mut b, &unsettled);
        assert_eq!(b.messages.len(), 1);
    }
}
