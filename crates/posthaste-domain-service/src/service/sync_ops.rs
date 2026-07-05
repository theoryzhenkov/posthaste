use std::sync::Arc;

use async_trait::async_trait;

use super::*;
use crate::{SyncChunkSink, SyncWriteStore};
use posthaste_domain_model::{MessageRecord, Operation, SyncBatch};

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
    /// Ids of messages with an un-acked optimistic op — the M35 durable guard's
    /// protected set (also passed to the store as `protected_message_ids` so a
    /// snapshot that *omits* such a message doesn't prune it as absent).
    unsettled: std::collections::HashSet<String>,
    /// The un-acked ops themselves, so a snapshot row for an unsettled message
    /// can be folded (server truth + pending effect) rather than dropped.
    unsettled_ops: Vec<Operation>,
    publish: &'a mut (dyn FnMut(&[DomainEvent]) + Send),
    applied_events: Vec<DomainEvent>,
    messages: Vec<MessageRecord>,
    mailbox_count: usize,
    deleted_imap_location_count: usize,
    deleted_message_count: usize,
    error: Option<ServiceError>,
}

/// Fold each un-acked local mutation over the provider snapshot rows — the M35
/// durable snapshot guard (D93), the principled successor to the P1/S2 hotfix
/// (which this supersedes). A provider snapshot, full or delta, is authoritative
/// for *server* state, but an outbox operation the server has not yet acked must
/// survive it: a message the user just flagged, whose flag hasn't round-tripped,
/// must not revert when a snapshot lands.
///
/// For every snapshot row whose message has an unsettled op, the row is replaced
/// by [`project_record`](super::message_queries::project_record)`(server_row,
/// unsettled_ops)` — server truth with the pending assertions re-layered on top.
/// This reuses the exact fold the settle write-back and read overlay use, so the
/// optimistic effect is defined once. A row that folds to removed (a pending
/// `Destroy`) is dropped from the upsert so the snapshot cannot resurrect a
/// locally-destroyed message; a server delete-delta for an unsettled message is
/// likewise dropped, the pending intent outranking it until it settles.
///
/// This composes with [`SyncWriteStore::apply_sync_batch_protected`]/
/// [`SyncWriteStore::reconcile_sync_protected`]: a full snapshot that *omits* an
/// unsettled message (a not-yet-uploaded local create, or a row that folded to
/// removed) would otherwise be pruned as "absent from remote" — the caller
/// passes this same `unsettled` set as `protected_message_ids` to exempt it from
/// that prune pass. Rows present in the snapshot are folded in-batch here and so
/// carry their own presence into the prune's remote set; only the omitted ones
/// rely on the exemption.
///
/// The ack gate is [`overlay_operations`](super::MailService::overlay_operations)
/// computed *after* the pre-observe FLUSH: a genuinely acked op has already been
/// settled and removed, so it is absent from `unsettled`/`unsettled_ops` and its
/// stale effect is *not* re-layered — the snapshot supersedes it cleanly.
fn guard_unsettled(
    batch: &mut SyncBatch,
    unsettled: &std::collections::HashSet<String>,
    unsettled_ops: &[Operation],
) -> Result<(), ServiceError> {
    if unsettled.is_empty() {
        return Ok(());
    }
    let mut folded = Vec::with_capacity(batch.messages.len());
    for message in std::mem::take(&mut batch.messages) {
        if unsettled.contains(message.id.as_str()) {
            // Server truth with the un-acked local mutation re-layered on top;
            // `None` means it folded to removed (pending Destroy) — leave it out
            // so the snapshot upsert cannot resurrect it.
            if let Some(record) =
                super::message_queries::project_record(message, unsettled_ops)?
            {
                folded.push(record);
            }
        } else {
            folded.push(message);
        }
    }
    batch.messages = folded;
    batch
        .deleted_message_ids
        .retain(|id| !unsettled.contains(id.as_str()));
    Ok(())
}

#[async_trait]
impl SyncChunkSink for ServiceSyncSink<'_> {
    async fn emit(&mut self, mut batch: SyncBatch) -> Result<(), GatewayError> {
        if let Err(error) = guard_unsettled(&mut batch, &self.unsettled, &self.unsettled_ops) {
            self.error = Some(error);
            return Err(GatewayError::Rejected(
                "sync chunk could not be reconciled against pending ops".to_string(),
            ));
        }
        // Offloaded (D63/M23b): `batch` is cloned once into the 'static
        // closure spawn_blocking requires; `batch` itself stays owned here for
        // the post-apply counts/append below.
        let sync_writer = self.sync_writer.clone();
        let account_id = self.account_id.clone();
        let unsettled = self.unsettled.clone();
        let owned_batch = batch.clone();
        let result = offload(move || {
            sync_writer.apply_sync_batch_protected(&account_id, &owned_batch, &unsettled)
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
        // outcome carries any reconciliation set for the final pass.
        //
        // Computed after FLUSH above, so just-settled (acked) ops are excluded
        // and the sync applies their authoritative effect. Reused below for the
        // final reconciliation pass so a streamed full-snapshot fallback (e.g.
        // JMAP `cannotCalculateChanges`) guards the same set of messages from
        // its prune-by-absence pass as each chunk did. The ops are carried
        // alongside the id set so each snapshot row can be folded (server truth
        // + pending effect), not merely excluded.
        let unsettled_ops = self.overlay_operations(account_id)?;
        let unsettled: std::collections::HashSet<String> = unsettled_ops
            .iter()
            .map(|operation| operation.entity.id.clone())
            .collect();
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
                unsettled: unsettled.clone(),
                unsettled_ops,
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
            let owned_unsettled = unsettled.clone();
            let reconcile_events = offload(move || {
                sync_writer.reconcile_sync_protected(
                    &owned_account_id,
                    &owned_reconciliation,
                    &owned_unsettled,
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
        // M35 durable snapshot guard: computed after FLUSH so acked ops are
        // excluded. The ops fold un-acked local effect over each snapshot row;
        // the id set is also passed to the protected apply below so a
        // full-snapshot `batch` doesn't prune a message it omits.
        let unsettled_ops = self.overlay_operations(account_id)?;
        let unsettled: std::collections::HashSet<String> = unsettled_ops
            .iter()
            .map(|operation| operation.entity.id.clone())
            .collect();
        guard_unsettled(&mut batch, &unsettled, &unsettled_ops)?;
        let sync_writer = self.sync_writer.clone();
        let owned_account_id = account_id.clone();
        let owned_batch = batch.clone();
        let owned_unsettled = unsettled.clone();
        events.extend(
            offload(move || {
                sync_writer.apply_sync_batch_protected(&owned_account_id, &owned_batch, &owned_unsettled)
            })
            .await?,
        );
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
    use posthaste_domain_model::{
        AccountId, Id, MailboxId, MessageId, MessageRecord, Operation, OperationEntity,
        OperationEntityKind, OperationId, OperationKind, OperationState, SetKeywordsCommand,
        SyncBatch,
    };
    use std::collections::HashSet;

    /// A snapshot row (server truth) for `id` with the given keywords + mailbox.
    fn server_row(id: &str, keywords: &[&str], mailbox: &str) -> MessageRecord {
        MessageRecord {
            id: MessageId::from(id),
            keywords: keywords.iter().map(|k| k.to_string()).collect(),
            mailbox_ids: vec![MailboxId::from(mailbox)],
            ..Default::default()
        }
    }

    fn batch(messages: Vec<MessageRecord>, deletes: &[&str], replace_all_messages: bool) -> SyncBatch {
        SyncBatch {
            mailboxes: Vec::new(),
            messages,
            imap_mailbox_states: Vec::new(),
            imap_message_locations: Vec::new(),
            deleted_imap_message_locations: Vec::new(),
            deleted_mailbox_ids: Vec::new(),
            absence_deleted_imap_message_locations: Vec::new(),
            absence_deleted_message_ids: Vec::new(),
            deleted_message_ids: deletes.iter().map(|id| MessageId::from(*id)).collect(),
            replace_all_mailboxes: false,
            replace_all_messages,
            cursors: Vec::new(),
        }
    }

    fn op(message_id: &str, kind: OperationKind, payload: serde_json::Value) -> Operation {
        Operation {
            id: OperationId::from(Id::generate().to_string()),
            account_id: AccountId::from("primary"),
            entity: OperationEntity {
                kind: OperationEntityKind::Message,
                id: message_id.to_string(),
            },
            kind,
            payload,
            state: OperationState::Pending,
            attempts: 0,
            last_error: None,
            depends_on: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn set_flagged(message_id: &str) -> Operation {
        op(
            message_id,
            OperationKind::SetKeywords,
            serde_json::to_value(SetKeywordsCommand {
                add: vec!["$flagged".to_string()],
                remove: Vec::new(),
            })
            .expect("serialize set-keywords payload"),
        )
    }

    fn keywords_of(batch: &SyncBatch, id: &str) -> HashSet<String> {
        batch
            .messages
            .iter()
            .find(|m| m.id.as_str() == id)
            .map(|m| m.keywords.iter().cloned().collect())
            .unwrap_or_default()
    }

    #[test]
    fn folds_the_pending_flag_over_the_server_row() {
        // THE M35 HEADLINE (D93): a snapshot that carries message-1 as server
        // truth (unflagged) must not revert an un-acked local flag. The row is
        // kept — server truth with the pending SetKeywords re-layered on top —
        // not dropped, so a legitimate server field would also survive.
        let unsettled = HashSet::from(["message-1".to_string()]);
        let ops = vec![set_flagged("message-1")];
        let mut b = batch(
            vec![
                server_row("message-1", &["$seen"], "inbox"),
                server_row("message-2", &[], "inbox"),
            ],
            &[],
            true,
        );
        guard_unsettled(&mut b, &unsettled, &ops).expect("fold succeeds");

        // message-1 is still present (not dropped) and now carries BOTH the
        // server keyword ($seen) and the un-acked local flag ($flagged).
        assert!(
            b.messages.iter().any(|m| m.id.as_str() == "message-1"),
            "the pending row survives the snapshot as a fold, not a drop",
        );
        assert_eq!(
            keywords_of(&b, "message-1"),
            HashSet::from(["$seen".to_string(), "$flagged".to_string()]),
            "server truth is applied AND the un-acked flag is re-layered on top",
        );
    }

    #[test]
    fn non_unsettled_rows_take_server_truth_unchanged() {
        // The ack gate: message-2 has no un-acked op, so the snapshot supersedes
        // it cleanly — no stale overlay folded in.
        let unsettled = HashSet::from(["message-1".to_string()]);
        let ops = vec![set_flagged("message-1")];
        let mut b = batch(
            vec![server_row("message-2", &["$seen"], "archive")],
            &[],
            true,
        );
        guard_unsettled(&mut b, &unsettled, &ops).expect("fold succeeds");
        assert_eq!(
            keywords_of(&b, "message-2"),
            HashSet::from(["$seen".to_string()]),
            "an acked/absent-from-pending message keeps exactly the server row",
        );
    }

    #[test]
    fn pending_destroy_drops_the_row_from_the_upsert() {
        // A pending Destroy folds to removed: the snapshot must not resurrect a
        // locally-destroyed message, so its row is dropped from the upsert (the
        // caller's protected set then keeps the prune pass off it too).
        let unsettled = HashSet::from(["message-1".to_string()]);
        let ops = vec![op(
            "message-1",
            OperationKind::Destroy,
            serde_json::Value::Null,
        )];
        let mut b = batch(vec![server_row("message-1", &[], "inbox")], &[], true);
        guard_unsettled(&mut b, &unsettled, &ops).expect("fold succeeds");
        assert!(
            b.messages.is_empty(),
            "the locally-destroyed message is not re-upserted from the snapshot",
        );
    }

    #[test]
    fn unsettled_message_is_dropped_from_server_deletes() {
        // A server delete-delta for an unsettled message is ignored: the pending
        // intent outranks it until it settles.
        let unsettled = HashSet::from(["message-1".to_string()]);
        let ops = vec![set_flagged("message-1")];
        let mut b = batch(Vec::new(), &["message-1", "message-3"], false);
        guard_unsettled(&mut b, &unsettled, &ops).expect("fold succeeds");
        assert_eq!(
            b.deleted_message_ids
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>(),
            vec!["message-3"],
            "only the unsettled message is spared the server delete",
        );
    }

    #[test]
    fn keeps_everything_when_nothing_is_unsettled() {
        let mut b = batch(
            vec![
                server_row("message-1", &["$seen"], "inbox"),
                server_row("message-2", &[], "inbox"),
            ],
            &["message-1"],
            false,
        );
        guard_unsettled(&mut b, &HashSet::new(), &[]).expect("no-op guard");
        assert_eq!(b.messages.len(), 2);
        assert_eq!(b.deleted_message_ids.len(), 1);
    }
}
