//! Tier-2 (runtime <-> provider) outbox engine: enqueue and flush.
//!
//! Callers enqueue an [`Operation`]; pending operations form a read-time overlay
//! and the flusher drains them to the provider, settling applied/failed outcomes,
//! reconciling temporary entity ids to provider ids, and emitting
//! `operation.settled` events.
//!
//! @spec docs/L1-outbox#operation-model
//! @spec docs/L1-outbox#state-machine

use super::message_queries::project_record;
use super::*;
use posthaste_domain_model::{MessageReadback, MessageRecord, MutationOutcome, SyncBatch};

/// Outcome of attempting to push one operation to the provider.
enum FlushError {
    /// The provider is unreachable or transiently failing; keep the op pending
    /// and stop draining (we are effectively offline).
    Transient(String),
    /// The op can never succeed as written (validation/unsupported); fail it.
    Permanent(String),
}

enum DependencyStatus {
    Satisfied,
    Waiting,
    Cancelled(String),
}

fn classify_gateway_error(error: GatewayError) -> FlushError {
    match error {
        GatewayError::Network(message) | GatewayError::Unavailable(message) => {
            FlushError::Transient(message)
        }
        // Auth failures are transient: they clear once the account re-authenticates.
        GatewayError::Auth => FlushError::Transient("authentication required".to_string()),
        GatewayError::StateMismatch => FlushError::Permanent("provider state diverged".to_string()),
        GatewayError::Rejected(message) => FlushError::Permanent(message),
        other => FlushError::Permanent(other.to_string()),
    }
}

/// Result of pushing one operation to the provider.
#[allow(clippy::large_enum_variant)]
enum Pushed {
    /// A non-message entity op (draft/send): settle and remove; an
    /// `assigned_entity_id` reconciles a temporary draft id to the provider id.
    Entity { assigned_entity_id: Option<String> },
    /// A message state assertion: settle now via the provider readback.
    /// `rejected` is `Some(reason)` when the provider rejected the change — the
    /// readback then carries the unchanged state, so the settle write reverts.
    Message {
        readback: Option<MessageReadback>,
        rejected: Option<String>,
    },
}

/// Normalize a message-mutation gateway result into a [`Pushed::Message`]:
/// `Ok` (accepted) and `MutationRejected` (rejected) both carry a readback and
/// settle in one path; only a transport error is a flush error (retry).
fn message_pushed(result: Result<MutationOutcome, GatewayError>) -> Result<Pushed, FlushError> {
    match result {
        Ok(outcome) => Ok(Pushed::Message {
            readback: outcome.message,
            rejected: None,
        }),
        Err(GatewayError::MutationRejected { readback, reason }) => Ok(Pushed::Message {
            readback: Some(*readback),
            rejected: Some(reason),
        }),
        Err(transport) => Err(classify_gateway_error(transport)),
    }
}

/// A single-message authoritative upsert batch (no cursor advance, no mailbox
/// changes) — the settle write reuses the sync write path for one record.
fn upsert_message_batch(record: MessageRecord) -> SyncBatch {
    SyncBatch {
        messages: vec![record],
        ..SyncBatch::default()
    }
}

/// A single-message delete batch (the readback folded to removed).
fn delete_message_batch(message_id: &MessageId) -> SyncBatch {
    SyncBatch {
        deleted_message_ids: vec![message_id.clone()],
        ..SyncBatch::default()
    }
}

impl MailService {
    /// Persist an operation. Idempotent on [`Operation::id`].
    ///
    /// @spec docs/L1-outbox#idempotency
    pub fn enqueue_operation(&self, operation: Operation) -> Result<Operation, ServiceError> {
        self.outbox
            .enqueue_operation(&operation)
            .map_err(Into::into)
    }

    /// All non-terminal operations for an account, oldest first. Used to hydrate
    /// optimistic state and surface pending/failed work.
    pub fn list_pending_operations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<Operation>, ServiceError> {
        self.outbox
            .list_pending_operations(account_id)
            .map_err(Into::into)
    }

    /// Remove a queued or failed outbox operation, giving the user an escape
    /// hatch for a dead op. An in-flight op is never yanked (its provider call
    /// may be mid-send). Discarding a failed op also unblocks its dependents: a
    /// missing dependency reads as satisfied, so a dependent no longer cancels.
    ///
    /// @spec docs/L1-outbox#state-machine
    pub fn discard_operation(&self, operation_id: &OperationId) -> Result<bool, ServiceError> {
        let Some(operation) = self.outbox.get_operation(operation_id)? else {
            return Ok(false);
        };
        if operation.state == OperationState::Inflight {
            return Err(GatewayError::Rejected(
                "cannot discard an in-flight operation".to_string(),
            )
            .into());
        }
        self.outbox.remove_operation(operation_id)?;
        Ok(true)
    }

    /// Re-arm a failed outbox operation to `pending` so the next flush
    /// re-attempts it (e.g. after the cause of the failure is fixed). Clears the
    /// recorded error. Only failed ops are retryable.
    ///
    /// @spec docs/L1-outbox#state-machine
    pub fn retry_operation(&self, operation_id: &OperationId) -> Result<bool, ServiceError> {
        let Some(operation) = self.outbox.get_operation(operation_id)? else {
            return Ok(false);
        };
        if operation.state != OperationState::Failed {
            return Err(GatewayError::Rejected(
                "only failed operations can be retried".to_string(),
            )
            .into());
        }
        self.outbox.update_operation_state(
            operation_id,
            OperationState::Pending,
            operation.attempts,
            None,
        )?;
        Ok(true)
    }

    /// Construct and enqueue an operation, capturing creation timestamps and
    /// ordering draft chains after the latest still-pending op for the same entity.
    ///
    /// @spec docs/L1-outbox#operation-model
    pub fn queue_operation(
        &self,
        account_id: &AccountId,
        entity: OperationEntity,
        kind: OperationKind,
        mut payload: serde_json::Value,
    ) -> Result<Operation, ServiceError> {
        let depends_on = if kind.is_state_assertion() {
            // State assertions coalesce instead of chaining: a new assertion
            // supersedes (or merges with) the pending assertion it replaces, so
            // the outbox holds the latest desired state per (entity, kind).
            self.coalesce_pending_assertions(account_id, &entity, kind, &mut payload)?;
            None
        } else {
            self.outbox
                .list_pending_operations(account_id)?
                .into_iter()
                .rfind(|existing| existing.entity == entity)
                .map(|existing| existing.id)
        };
        let now =
            now_iso8601().map_err(|error| ServiceError::from(GatewayError::Rejected(error)))?;
        let operation = Operation {
            id: OperationId::from(Id::generate().to_string()),
            account_id: account_id.clone(),
            entity,
            kind,
            payload,
            state: OperationState::Pending,
            attempts: 0,
            last_error: None,
            depends_on,
            created_at: now.clone(),
            updated_at: now,
        };
        self.enqueue_operation(operation)
    }

    /// Supersede pending assertions for an entity per the coalescing policy.
    ///
    /// Only still-`Pending` assertions are touched; an inflight or failed op is
    /// left as-is. `destroy` supersedes every pending assertion for the entity,
    /// `replaceMailboxes` supersedes the pending `replaceMailboxes`, and
    /// `setKeywords` merges its add/remove deltas with the pending `setKeywords`.
    ///
    /// @spec docs/L1-outbox#operation-model
    fn coalesce_pending_assertions(
        &self,
        account_id: &AccountId,
        entity: &OperationEntity,
        kind: OperationKind,
        payload: &mut serde_json::Value,
    ) -> Result<(), ServiceError> {
        let superseded: Vec<Operation> = self
            .outbox
            .list_pending_operations(account_id)?
            .into_iter()
            .filter(|existing| {
                existing.entity == *entity
                    && existing.kind.is_state_assertion()
                    && existing.state == OperationState::Pending
                    && match kind {
                        OperationKind::Destroy => true,
                        OperationKind::ReplaceMailboxes => {
                            existing.kind == OperationKind::ReplaceMailboxes
                        }
                        OperationKind::SetKeywords => existing.kind == OperationKind::SetKeywords,
                        _ => false,
                    }
            })
            .collect();
        for existing in superseded {
            if kind == OperationKind::SetKeywords && existing.kind == OperationKind::SetKeywords {
                *payload = merge_set_keywords(&existing.payload, payload)?;
            }
            self.outbox.remove_operation(&existing.id)?;
        }
        Ok(())
    }

    /// Enqueue an outgoing message local-first.
    ///
    /// The send is queued and flushed to the provider on the next connectivity
    /// window; the caller does not need a live gateway. A unique entity id makes
    /// the operation its own idempotency unit so it never coalesces and is sent
    /// at most once (see the send-once recovery in [`Self::flush_account`]).
    ///
    /// @spec docs/L1-outbox#operation-model
    pub fn enqueue_send(
        &self,
        account_id: &AccountId,
        request: SendMessageRequest,
    ) -> Result<Operation, ServiceError> {
        let payload = encode_payload(request, "send request")?;
        self.queue_operation(
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Message,
                id: format!("send-{}", Id::generate()),
            },
            OperationKind::Send,
            payload,
        )
    }

    /// Save a draft local-first: enqueue a draft create/update operation.
    ///
    /// `draft_id` is `None` for a brand-new draft (a temporary entity id is
    /// minted and reconciled to the provider id on first flush) or the existing
    /// draft's id for an edit. The optimistic draft lives in the outbox until it
    /// flushes to the provider's Drafts mailbox; consumers render pending draft
    /// operations to show it while offline.
    ///
    /// @spec docs/L1-outbox#operation-model
    /// @spec docs/L1-outbox#temp-id-reconciliation
    pub fn save_draft(
        &self,
        account_id: &AccountId,
        draft_key: Option<MessageId>,
        mut request: SendMessageRequest,
    ) -> Result<Operation, ServiceError> {
        // `draft_key` is a stable, client-chosen handle for the draft. The alias
        // maps it to the entity id currently representing the draft (a temporary
        // id before the first flush, a provider id after), so the client can keep
        // using the same key across flushes without creating duplicate drafts.
        let key = draft_key
            .map(|id| id.to_string())
            .unwrap_or_else(|| format!("draft-local-{}", Id::generate()));
        // Stamp the stable identity into the payload so the gateway writes it as
        // the `X-Posthaste-Draft-Id` header. The header survives the provider id
        // rotation a JMAP draft update causes and is read back on resume, so the
        // client always keys by a stable value.
        request.draft_id = Some(key.clone());
        let (entity_id, kind) = match self.outbox.resolve_draft_entity(account_id, &key)? {
            Some(entity_id) => (entity_id, OperationKind::DraftUpdate),
            None => {
                self.outbox.set_draft_alias(account_id, &key, &key)?;
                // A key with no alias that already names an existing draft
                // message is a draft resumed by its (rotating) provider id — a
                // legacy draft saved before stable ids, or one created
                // elsewhere. Replace it in place instead of creating a
                // duplicate; this also bootstraps a stable header onto it.
                let kind = if self.draft_message_exists(account_id, &key)? {
                    OperationKind::DraftUpdate
                } else {
                    OperationKind::DraftCreate
                };
                (key.clone(), kind)
            }
        };
        let payload = encode_payload(request, "draft request")?;
        self.queue_operation(
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Draft,
                id: entity_id,
            },
            kind,
            payload,
        )
    }

    /// Delete a draft local-first: enqueue a draft delete operation for the
    /// draft's current id (temporary or provider-assigned).
    ///
    /// @spec docs/L1-outbox#operation-model
    pub fn delete_draft(
        &self,
        account_id: &AccountId,
        draft_key: MessageId,
    ) -> Result<Operation, ServiceError> {
        let key = draft_key.to_string();
        let entity_id = self
            .outbox
            .resolve_draft_entity(account_id, &key)?
            .unwrap_or_else(|| key.clone());
        let operation = self.queue_operation(
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Draft,
                id: entity_id,
            },
            OperationKind::DraftDelete,
            serde_json::json!({}),
        )?;
        self.outbox.remove_draft_alias(account_id, &key)?;
        Ok(operation)
    }

    /// Whether `draft_key` names a message already in the projection — i.e. a
    /// draft being resumed by its provider id rather than a freshly minted
    /// local key. Used to edit such a draft in place instead of duplicating it.
    /// Mailbox membership is the light existence proxy: a draft always sits in
    /// the Drafts mailbox.
    fn draft_message_exists(
        &self,
        account_id: &AccountId,
        draft_key: &str,
    ) -> Result<bool, ServiceError> {
        let message_id = MessageId::from(draft_key);
        Ok(!self
            .message_mailboxes
            .get_message_mailboxes(account_id, &message_id)?
            .is_empty())
    }

    /// Flush all flushable operations for an account to the provider, returning
    /// the `operation.settled` events to publish.
    ///
    /// Stops draining on the first transient (offline) failure so later ops are
    /// retried together on the next connectivity window. Per-entity ordering is
    /// preserved: an op whose dependency has not yet applied is skipped this pass.
    ///
    /// @spec docs/L1-outbox#state-machine
    pub async fn flush_account(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let queued = self.outbox.list_flushable_operations(account_id)?;
        let mut events = Vec::new();
        for snapshot in queued {
            // Re-fetch fresh: an earlier op in this pass may have reconciled this
            // op's entity id (temp -> provider) or changed its state.
            let Some(operation) = self.outbox.get_operation(&snapshot.id)? else {
                continue;
            };
            if !operation.state.is_flushable() {
                continue;
            }
            // Send-once: a `send` found already `inflight` was interrupted after
            // a prior flush began, so the message may already have left the
            // provider. SMTP is not idempotent, so never auto-resend it; fail it
            // terminally and let the user decide rather than risk a duplicate.
            //
            // @spec docs/L1-outbox#operation-model
            if operation.kind == OperationKind::Send && operation.state == OperationState::Inflight
            {
                let message =
                    "send interrupted mid-flush; not retried to avoid a duplicate send".to_string();
                self.outbox.update_operation_state(
                    &operation.id,
                    OperationState::Failed,
                    operation.attempts,
                    Some(&message),
                )?;
                let settlement = OperationSettlement {
                    id: operation.id.clone(),
                    outcome: OperationOutcome::Failed,
                    assigned_entity_id: None,
                    error: Some(message),
                };
                events.push(self.emit_settlement(account_id, &operation, &settlement)?);
                if let Some(correction) =
                    self.emit_failure_base_correction(account_id, &operation)?
                {
                    events.push(correction);
                }
                continue;
            }
            match self.dependency_status(&operation)? {
                DependencyStatus::Satisfied => {}
                DependencyStatus::Waiting => continue,
                DependencyStatus::Cancelled(message) => {
                    self.outbox.update_operation_state(
                        &operation.id,
                        OperationState::Failed,
                        operation.attempts,
                        Some(&message),
                    )?;
                    let settlement = OperationSettlement {
                        id: operation.id.clone(),
                        outcome: OperationOutcome::Failed,
                        assigned_entity_id: None,
                        error: Some(message),
                    };
                    events.push(self.emit_settlement(account_id, &operation, &settlement)?);
                    if let Some(correction) =
                        self.emit_failure_base_correction(account_id, &operation)?
                    {
                        events.push(correction);
                    }
                    continue;
                }
            }
            self.outbox.update_operation_state(
                &operation.id,
                OperationState::Inflight,
                operation.attempts,
                operation.last_error.as_deref(),
            )?;
            match self.push_operation(account_id, &operation, gateway).await {
                Ok(Pushed::Entity { assigned_entity_id }) => {
                    if let Some(new_id) = assigned_entity_id.as_deref() {
                        if new_id != operation.entity.id {
                            self.outbox.reconcile_operation_entity_id(
                                account_id,
                                &operation.entity.id,
                                new_id,
                            )?;
                            // Keep the stable client draft key pointed at the
                            // live provider draft.
                            self.outbox.update_draft_alias_entity(
                                account_id,
                                &operation.entity.id,
                                new_id,
                            )?;
                        }
                    }
                    // Entity ops (drafts/sends) are not folded into message
                    // reads, so they settle and prune on flush.
                    let settlement = OperationSettlement {
                        id: operation.id.clone(),
                        outcome: OperationOutcome::Applied,
                        assigned_entity_id,
                        error: None,
                    };
                    events.push(self.emit_settlement(account_id, &operation, &settlement)?);
                    self.outbox.remove_operation(&operation.id)?;
                }
                Ok(Pushed::Message { readback, rejected }) => {
                    // Settle now from the provider readback: remove the op, fold
                    // the remaining unsettled assertions over the readback, and
                    // write canonical — the settle write reverts a rejected change
                    // (its readback is the unchanged row) and emits the recompute.
                    //
                    // @spec docs/eph/DESIGN-L2-optimistic-projection#3-the-runtime-write-through-mechanics
                    events.extend(
                        self.settle_message_operation(account_id, &operation, readback, rejected)?,
                    );
                }
                Err(FlushError::Transient(message)) => {
                    self.outbox.update_operation_state(
                        &operation.id,
                        OperationState::Pending,
                        operation.attempts + 1,
                        Some(&message),
                    )?;
                    // Offline: stop draining; the rest retries next window.
                    break;
                }
                Err(FlushError::Permanent(message)) => {
                    self.outbox.update_operation_state(
                        &operation.id,
                        OperationState::Failed,
                        operation.attempts + 1,
                        Some(&message),
                    )?;
                    let settlement = OperationSettlement {
                        id: operation.id.clone(),
                        outcome: OperationOutcome::Failed,
                        assigned_entity_id: None,
                        error: Some(message),
                    };
                    events.push(self.emit_settlement(account_id, &operation, &settlement)?);
                    if let Some(correction) =
                        self.emit_failure_base_correction(account_id, &operation)?
                    {
                        events.push(correction);
                    }
                }
            }
        }
        Ok(events)
    }

    /// Whether the operation this one depends on has applied, is still waiting,
    /// or failed and should cancel this dependent.
    fn dependency_status(&self, operation: &Operation) -> Result<DependencyStatus, ServiceError> {
        let Some(dependency) = &operation.depends_on else {
            return Ok(DependencyStatus::Satisfied);
        };
        match self.outbox.get_operation(dependency)? {
            // Applied operations are pruned, so a missing dependency is satisfied.
            None => Ok(DependencyStatus::Satisfied),
            Some(dep) if dep.state == OperationState::Applied => Ok(DependencyStatus::Satisfied),
            Some(dep) if dep.state == OperationState::Failed => Ok(DependencyStatus::Cancelled(
                format!("dependency {} failed", dep.id.as_str()),
            )),
            Some(_) => Ok(DependencyStatus::Waiting),
        }
    }

    /// Push a single operation to the provider, mapping the result to a
    /// settlement or a typed flush error.
    async fn push_operation(
        &self,
        account_id: &AccountId,
        operation: &Operation,
        gateway: &dyn MailGateway,
    ) -> Result<Pushed, FlushError> {
        match operation.kind {
            OperationKind::DraftCreate => {
                let request = parse_payload::<SendMessageRequest>(operation)?;
                let new_id = gateway
                    .save_draft(account_id, &request, None)
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: Some(new_id.to_string()),
                })
            }
            OperationKind::DraftUpdate => {
                let request = parse_payload::<SendMessageRequest>(operation)?;
                let replace = MessageId::from(operation.entity.id.as_str());
                let new_id = gateway
                    .save_draft(account_id, &request, Some(&replace))
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: Some(new_id.to_string()),
                })
            }
            OperationKind::DraftDelete => {
                let target = MessageId::from(operation.entity.id.as_str());
                gateway
                    .delete_draft(account_id, &target)
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: None,
                })
            }
            OperationKind::Send => {
                let request = parse_payload::<SendMessageRequest>(operation)?;
                gateway
                    .send_message(account_id, &request)
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: None,
                })
            }
            OperationKind::SetKeywords => {
                let command = parse_payload::<SetKeywordsCommand>(operation)?;
                let target = MessageId::from(operation.entity.id.as_str());
                message_pushed(
                    gateway
                        .set_keywords(account_id, &target, None, &command)
                        .await,
                )
            }
            OperationKind::ReplaceMailboxes => {
                let command = parse_payload::<ReplaceMailboxesCommand>(operation)?;
                let target = MessageId::from(operation.entity.id.as_str());
                message_pushed(
                    gateway
                        .replace_mailboxes(account_id, &target, None, &command.mailbox_ids)
                        .await,
                )
            }
            OperationKind::Destroy => {
                let target = MessageId::from(operation.entity.id.as_str());
                message_pushed(gateway.destroy_message(account_id, &target, None).await)
            }
        }
    }

    /// Settle a message state assertion from the provider readback: remove the
    /// op, fold the remaining unsettled assertions for the message over the
    /// readback (the new base), and write canonical via the sync write path.
    /// `Removed`/folded-to-removed deletes the row; a `None` readback (a gateway
    /// that did not read back, e.g. IMAP) leaves the optimistic write for a later
    /// sync to reconcile.
    ///
    /// @spec docs/eph/DESIGN-L2-optimistic-projection#3-the-runtime-write-through-mechanics
    fn settle_message_operation(
        &self,
        account_id: &AccountId,
        operation: &Operation,
        readback: Option<MessageReadback>,
        rejected: Option<String>,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let message_id = MessageId::from(operation.entity.id.as_str());
        // Remove the settled op FIRST so `remaining` excludes it and the
        // canonical write is no longer guarded as unsettled (S3).
        self.outbox.remove_operation(&operation.id)?;
        let remaining: Vec<Operation> = self
            .outbox
            .list_unsettled_operations(account_id)?
            .into_iter()
            .filter(|op| {
                op.entity.kind == OperationEntityKind::Message
                    && op.kind.is_state_assertion()
                    && op.entity.id == message_id.as_str()
            })
            .collect();
        let mut events = Vec::new();
        let batch = match readback {
            Some(MessageReadback::Present(record)) => match project_record(record, &remaining)? {
                Some(record) => Some(upsert_message_batch(record)),
                None => Some(delete_message_batch(&message_id)),
            },
            Some(MessageReadback::Removed) => Some(delete_message_batch(&message_id)),
            None => None,
        };
        if let Some(batch) = batch {
            events.extend(self.sync_writer.apply_sync_batch(account_id, &batch)?);
        }
        let settlement = OperationSettlement {
            id: operation.id.clone(),
            outcome: if rejected.is_some() {
                OperationOutcome::Failed
            } else {
                OperationOutcome::Applied
            },
            assigned_entity_id: None,
            error: rejected,
        };
        events.push(self.emit_settlement(account_id, operation, &settlement)?);
        Ok(events)
    }

    /// When a message state-assertion op fails, its optimistic effect leaves the
    /// read overlay (a `Failed` op is no longer folded), but nothing tells the
    /// served views to recompute, so they keep showing the now-reverted
    /// optimistic value. Re-assert the message's authoritative state as a
    /// `message.updated` so every view recomputes back to truth — the failure
    /// correction arrives as a base update, not a settlement-only signal.
    /// Returns `None` for ops that don't fold into message reads (drafts/sends),
    /// which surface via `operation.settled` instead.
    ///
    /// @spec docs/replication/L1#corrections-as-base-updates
    /// @spec docs/replication/L1#permanent-failure-surfaces
    fn emit_failure_base_correction(
        &self,
        account_id: &AccountId,
        operation: &Operation,
    ) -> Result<Option<DomainEvent>, ServiceError> {
        if operation.entity.kind != OperationEntityKind::Message
            || !operation.kind.is_state_assertion()
        {
            return Ok(None);
        }
        let message_id = MessageId::from(operation.entity.id.as_str());
        let event = self.events.append_event(
            account_id,
            EVENT_TOPIC_MESSAGE_UPDATED,
            None,
            Some(&message_id),
            json!({
                "messageId": message_id.as_str(),
                // The read already reflects the reverted state (the failed op
                // left the overlay); both dimensions a state assertion can touch
                // are flagged so any served view (list/detail) recomputes.
                "changes": { "keywords": true, "mailboxes": true },
                "reverted": true,
                "resources": [
                    { "kind": "message", "operation": "reverted", "accountId": account_id.as_str() },
                ],
            }),
        )?;
        Ok(Some(event))
    }

    fn emit_settlement(
        &self,
        account_id: &AccountId,
        operation: &Operation,
        settlement: &OperationSettlement,
    ) -> Result<DomainEvent, ServiceError> {
        let message_id = match operation.entity.kind {
            OperationEntityKind::Message | OperationEntityKind::Draft => Some(MessageId::from(
                settlement
                    .assigned_entity_id
                    .as_deref()
                    .unwrap_or(operation.entity.id.as_str()),
            )),
        };
        let payload = encode_payload(settlement, "operation settlement")?;
        self.events
            .append_event(
                account_id,
                EVENT_TOPIC_OPERATION_SETTLED,
                None,
                message_id.as_ref(),
                payload,
            )
            .map_err(Into::into)
    }
}

fn parse_payload<T: serde::de::DeserializeOwned>(operation: &Operation) -> Result<T, FlushError> {
    serde_json::from_value(operation.payload.clone()).map_err(|error| {
        FlushError::Permanent(format!("invalid {:?} payload: {error}", operation.kind))
    })
}

/// Merge two `setKeywords` deltas into one, with the newer delta taking
/// precedence where add and remove disagree on the same keyword.
///
/// @spec docs/L1-outbox#operation-model
fn merge_set_keywords(
    older: &serde_json::Value,
    newer: &serde_json::Value,
) -> Result<serde_json::Value, ServiceError> {
    let parse = |value: &serde_json::Value| {
        decode_payload::<SetKeywordsCommand>(value.clone(), "setKeywords payload to coalesce")
    };
    let older = parse(older)?;
    let newer = parse(newer)?;
    let new_add: std::collections::BTreeSet<&String> = newer.add.iter().collect();
    let new_remove: std::collections::BTreeSet<&String> = newer.remove.iter().collect();
    let mut add: Vec<String> = older
        .add
        .iter()
        .chain(newer.add.iter())
        .filter(|keyword| !new_remove.contains(keyword))
        .cloned()
        .collect();
    add.sort();
    add.dedup();
    let mut remove: Vec<String> = older
        .remove
        .iter()
        .chain(newer.remove.iter())
        .filter(|keyword| !new_add.contains(keyword))
        .cloned()
        .collect();
    remove.sort();
    remove.dedup();
    encode_payload(SetKeywordsCommand { add, remove }, "merged setKeywords")
}
