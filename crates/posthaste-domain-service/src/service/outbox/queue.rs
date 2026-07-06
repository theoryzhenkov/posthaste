//! Operation queueing: enqueue/list/discard/retry, construction with
//! dependency chaining, and state-assertion coalescing.

use crate::service::*;

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

    /// Re-arm a failed or dispatch-uncertain outbox operation to `pending` so
    /// the next flush re-attempts it (e.g. after the cause of the failure is
    /// fixed, or the user confirms a parked send should be re-dispatched).
    /// Clears the recorded error. A parked send is re-dispatched under the same
    /// idempotency identity (D84/D85), so a re-forward of one that already
    /// committed is deduplicated rather than duplicated on JMAP (best-effort on
    /// SMTP — O5).
    ///
    /// @spec docs/L1-outbox#state-machine
    /// @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
    pub fn retry_operation(&self, operation_id: &OperationId) -> Result<bool, ServiceError> {
        let Some(operation) = self.outbox.get_operation(operation_id)? else {
            return Ok(false);
        };
        if !matches!(
            operation.state,
            OperationState::Failed | OperationState::DispatchUncertain
        ) {
            return Err(GatewayError::Rejected(
                "only failed or dispatch-uncertain operations can be retried".to_string(),
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
