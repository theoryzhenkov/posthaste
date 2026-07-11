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
    /// hatch for a dead op — and the CANCEL path for a scheduled (undo-send /
    /// send-later) send. An in-flight op is never yanked (its provider call
    /// may be mid-send). Discarding a failed op also unblocks its dependents: a
    /// missing dependency reads as satisfied, so a dependent no longer cancels.
    ///
    /// Cancel-vs-flush has exactly one winner: the removal is a single guarded
    /// statement (`remove_operation_unless_inflight`) racing the flusher's
    /// guarded claim (`claim_operation_for_flush`) — whichever write the store
    /// serializes first wins, the loser observes nothing to act on. There is no
    /// check-then-remove window: if this returns `Ok(true)` the op can never be
    /// pushed; if the flusher claimed first, this surfaces the in-flight error
    /// (the op is being — or has been — sent).
    ///
    /// @spec docs/L1-outbox#state-machine
    pub fn discard_operation(&self, operation_id: &OperationId) -> Result<bool, ServiceError> {
        if self.outbox.remove_operation_unless_inflight(operation_id)? {
            return Ok(true);
        }
        // Nothing removed: either the op is gone (settled/never existed — the
        // pre-existing `Ok(false)`), or it is in flight and must not be yanked.
        match self.outbox.get_operation(operation_id)? {
            Some(operation) if operation.state == OperationState::Inflight => Err(
                GatewayError::Rejected("cannot discard an in-flight operation".to_string()).into(),
            ),
            _ => Ok(false),
        }
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
    #[allow(clippy::too_many_arguments)]
    pub fn queue_operation(
        &self,
        account_id: &AccountId,
        entity: OperationEntity,
        kind: OperationKind,
        mut payload: serde_json::Value,
        send_at: Option<String>,
        hold_until_mono: Option<i64>,
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
            send_at,
            hold_until_mono,
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
    /// A `send_at` on the request (undo-send / send-later — one mechanism) is
    /// validated and normalized here (canonical UTC whole-second RFC 3339;
    /// invalid input rejects the request; a PAST time is accepted and simply
    /// already due — the pinned choice) and stamped on the operation, which
    /// then rests `pending` — visible, cancelable via
    /// [`Self::discard_operation`] — until due. Persisted, so it survives
    /// restart. LOCAL-FIRST: this is not a server-side schedule; the send
    /// fires on the first flush window at/after `send_at` with the app
    /// running + online.
    ///
    /// @spec docs/L1-outbox#operation-model
    pub fn enqueue_send(
        &self,
        account_id: &AccountId,
        mut request: SendMessageRequest,
    ) -> Result<Operation, ServiceError> {
        // D152: an undo hold is a DURATION stamped on the daemon's monotonic
        // clock (the same clock that later judges it); `send_at` then degrades
        // to display metadata and is NOT stored (it must never gate the
        // flush — that was the nightly cross-clock P0). Send-later keeps the
        // normalized wall target, judged against a re-sampled wall clock.
        let undo_window = request.undo_window_seconds.take();
        let raw_send_at = request.send_at.take();
        let (send_at, hold_until_mono) = match undo_window {
            Some(window) => (
                None,
                Some(super::schedule::monotonic_now_secs() + i64::from(window)),
            ),
            None => (
                raw_send_at
                    .map(|raw| {
                        super::schedule::normalize_send_at(&raw)
                            .map_err(|error| ServiceError::from(GatewayError::Rejected(error)))
                    })
                    .transpose()?,
                None,
            ),
        };
        // The normalized hold lives on the OPERATION (the flush gate reads the
        // indexed column); it is dropped from the payload so an immediate
        // send's payload — and the bytes the gateway sees — stay identical to
        // the pre-feature shape.
        let payload = encode_payload(request, "send request")?;
        self.queue_operation(
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Message,
                id: format!("send-{}", Id::generate()),
            },
            OperationKind::Send,
            payload,
            send_at,
            hold_until_mono,
        )
    }

    /// Whether any scheduled send is due (`send_at <= now`) and still queued —
    /// the scheduler tick's probe. `true` tells the caller to trigger a flush
    /// sync so the due send fires promptly instead of waiting for the next
    /// poll window. Uses the same monotonic-anchored clock the flush gate
    /// compares against, so probe and gate can never disagree on due-ness.
    pub fn has_due_scheduled_sends(&self, account_id: &AccountId) -> Result<bool, ServiceError> {
        let wall_now = super::schedule::wall_now_rfc3339()
            .map_err(|error| ServiceError::from(GatewayError::Rejected(error)))?;
        let mono_now = super::schedule::monotonic_now_secs();
        Ok(self
            .outbox
            .count_due_scheduled_sends(account_id, &wall_now, mono_now)?
            > 0)
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
