//! Tier-2 (runtime <-> provider) outbox engine: enqueue and flush.
//!
//! Callers apply a command locally and enqueue an [`Operation`]; the flusher
//! drains flushable operations in order, pushes each to the provider, and
//! settles it (applied / conflicted / failed), reconciling temporary entity ids
//! to provider ids and emitting `operation.settled` events.
//!
//! @spec docs/L1-outbox#operation-model
//! @spec docs/L1-outbox#state-machine

use super::*;

/// Outcome of attempting to push one operation to the provider.
enum FlushError {
    /// The provider is unreachable or transiently failing; keep the op pending
    /// and stop draining (we are effectively offline).
    Transient(String),
    /// The provider state diverged from the op's base; resolve per policy.
    Conflict(String),
    /// The op can never succeed as written (validation/unsupported); fail it.
    Permanent(String),
}

fn classify_gateway_error(error: GatewayError) -> FlushError {
    match error {
        GatewayError::Network(message) | GatewayError::Unavailable(message) => {
            FlushError::Transient(message)
        }
        // Auth failures are transient: they clear once the account re-authenticates.
        GatewayError::Auth => FlushError::Transient("authentication required".to_string()),
        GatewayError::StateMismatch => FlushError::Conflict("provider state diverged".to_string()),
        GatewayError::Rejected(message) => FlushError::Permanent(message),
        other => FlushError::Permanent(other.to_string()),
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

    /// Construct and enqueue an operation, capturing creation timestamps and
    /// ordering it after the latest still-pending op for the same entity.
    ///
    /// @spec docs/L1-outbox#operation-model
    pub fn queue_operation(
        &self,
        account_id: &AccountId,
        entity: OperationEntity,
        kind: OperationKind,
        payload: serde_json::Value,
        base_cursor: Option<String>,
    ) -> Result<Operation, ServiceError> {
        let depends_on = self
            .outbox
            .list_pending_operations(account_id)?
            .into_iter()
            .rfind(|existing| existing.entity == entity)
            .map(|existing| existing.id);
        let now =
            now_iso8601().map_err(|error| ServiceError::from(GatewayError::Rejected(error)))?;
        let operation = Operation {
            id: OperationId::from(Id::generate().to_string()),
            account_id: account_id.clone(),
            entity,
            kind,
            payload,
            base_cursor,
            state: OperationState::Pending,
            attempts: 0,
            last_error: None,
            depends_on,
            created_at: now.clone(),
            updated_at: now,
        };
        self.enqueue_operation(operation)
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
        draft_id: Option<MessageId>,
        request: SendMessageRequest,
    ) -> Result<Operation, ServiceError> {
        let (entity_id, kind) = match draft_id {
            Some(id) => (id.to_string(), OperationKind::DraftUpdate),
            None => (
                format!("draft-local-{}", Id::generate()),
                OperationKind::DraftCreate,
            ),
        };
        let payload = serde_json::to_value(request).map_err(|error| {
            ServiceError::from(GatewayError::Rejected(format!(
                "failed to serialize draft request: {error}"
            )))
        })?;
        self.queue_operation(
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Draft,
                id: entity_id,
            },
            kind,
            payload,
            None,
        )
    }

    /// Delete a draft local-first: enqueue a draft delete operation for the
    /// draft's current id (temporary or provider-assigned).
    ///
    /// @spec docs/L1-outbox#operation-model
    pub fn delete_draft(
        &self,
        account_id: &AccountId,
        draft_id: MessageId,
    ) -> Result<Operation, ServiceError> {
        self.queue_operation(
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Draft,
                id: draft_id.to_string(),
            },
            OperationKind::DraftDelete,
            serde_json::json!({}),
            None,
        )
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
            if !self.dependency_satisfied(&operation)? {
                continue;
            }
            self.outbox.update_operation_state(
                &operation.id,
                OperationState::Inflight,
                operation.attempts,
                operation.last_error.as_deref(),
            )?;
            match self.push_operation(account_id, &operation, gateway).await {
                Ok(settlement) => {
                    if let Some(new_id) = settlement.assigned_entity_id.as_deref() {
                        if new_id != operation.entity.id {
                            self.outbox.reconcile_operation_entity_id(
                                account_id,
                                &operation.entity.id,
                                new_id,
                            )?;
                        }
                    }
                    self.outbox.update_operation_state(
                        &operation.id,
                        OperationState::Applied,
                        operation.attempts + 1,
                        None,
                    )?;
                    events.push(self.emit_settlement(account_id, &operation, &settlement)?);
                    self.outbox.remove_operation(&operation.id)?;
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
                Err(FlushError::Conflict(message)) => {
                    self.outbox.update_operation_state(
                        &operation.id,
                        OperationState::Conflicted,
                        operation.attempts + 1,
                        Some(&message),
                    )?;
                    let settlement = OperationSettlement {
                        id: operation.id.clone(),
                        outcome: OperationOutcome::Conflicted,
                        assigned_entity_id: None,
                        error: Some(message),
                    };
                    events.push(self.emit_settlement(account_id, &operation, &settlement)?);
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
                }
            }
        }
        Ok(events)
    }

    /// Whether the operation this one depends on has already applied (been
    /// pruned) so it is safe to flush now.
    fn dependency_satisfied(&self, operation: &Operation) -> Result<bool, ServiceError> {
        let Some(dependency) = &operation.depends_on else {
            return Ok(true);
        };
        // A dependency is satisfied once it is no longer present (applied ops are
        // pruned) or has reached a terminal state.
        match self.outbox.get_operation(dependency)? {
            None => Ok(true),
            Some(dep) => Ok(dep.state.is_terminal()),
        }
    }

    /// Push a single operation to the provider, mapping the result to a
    /// settlement or a typed flush error.
    async fn push_operation(
        &self,
        account_id: &AccountId,
        operation: &Operation,
        gateway: &dyn MailGateway,
    ) -> Result<OperationSettlement, FlushError> {
        let applied = |assigned: Option<String>| OperationSettlement {
            id: operation.id.clone(),
            outcome: OperationOutcome::Applied,
            assigned_entity_id: assigned,
            error: None,
        };
        match operation.kind {
            OperationKind::DraftCreate => {
                let request = parse_payload::<SendMessageRequest>(operation)?;
                let new_id = gateway
                    .save_draft(account_id, &request, None)
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(applied(Some(new_id.to_string())))
            }
            OperationKind::DraftUpdate => {
                let request = parse_payload::<SendMessageRequest>(operation)?;
                let replace = MessageId::from(operation.entity.id.as_str());
                let new_id = gateway
                    .save_draft(account_id, &request, Some(&replace))
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(applied(Some(new_id.to_string())))
            }
            OperationKind::DraftDelete => {
                let target = MessageId::from(operation.entity.id.as_str());
                gateway
                    .delete_draft(account_id, &target)
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(applied(None))
            }
            OperationKind::Send => {
                let request = parse_payload::<SendMessageRequest>(operation)?;
                gateway
                    .send_message(account_id, &request)
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(applied(None))
            }
            OperationKind::SetKeywords => {
                let command = parse_payload::<SetKeywordsCommand>(operation)?;
                let target = MessageId::from(operation.entity.id.as_str());
                gateway
                    .set_keywords(
                        account_id,
                        &target,
                        operation.base_cursor.as_deref(),
                        &command,
                    )
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(applied(None))
            }
            OperationKind::ReplaceMailboxes => {
                let command = parse_payload::<ReplaceMailboxesCommand>(operation)?;
                let target = MessageId::from(operation.entity.id.as_str());
                gateway
                    .replace_mailboxes(
                        account_id,
                        &target,
                        operation.base_cursor.as_deref(),
                        &command.mailbox_ids,
                    )
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(applied(None))
            }
            OperationKind::Destroy => {
                let target = MessageId::from(operation.entity.id.as_str());
                gateway
                    .destroy_message(account_id, &target, operation.base_cursor.as_deref())
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(applied(None))
            }
        }
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
        let payload = serde_json::to_value(settlement).map_err(|error| {
            ServiceError::from(GatewayError::Rejected(format!(
                "failed to serialize operation settlement: {error}"
            )))
        })?;
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
