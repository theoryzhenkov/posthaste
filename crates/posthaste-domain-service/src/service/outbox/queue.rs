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

    /// One outbox operation by id, in any state the outbox still holds
    /// (retired operations are gone).
    pub fn get_operation(
        &self,
        operation_id: &OperationId,
    ) -> Result<Option<Operation>, ServiceError> {
        self.outbox.get_operation(operation_id).map_err(Into::into)
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
    /// may be mid-send).
    ///
    /// Cancel-vs-flush has exactly one winner: the removal is a single guarded
    /// statement (`remove_operation_unless_inflight`) racing the flusher's
    /// guarded claim (`claim_operation_for_flush`) — whichever write the store
    /// serializes first wins, the loser observes nothing to act on. There is no
    /// check-then-remove window: if this returns `Ok(Some(_))` the op can never
    /// be pushed; if the flusher claimed first, this surfaces the in-flight
    /// error (the op is being — or has been — sent).
    ///
    /// Returns `Some(events)` when the op was removed (`None` = nothing to
    /// remove; a settled `applied` op counts as nothing to remove — it stays
    /// in the log until causal truncation and its mutation stands).
    ///
    /// Discarding a CONTENT op re-derives its authored row: removing the op is
    /// the whole of the cleanup (the derived row cannot outlive its op). For a
    /// draft save the row simply vanishes; for a SEND the provisional Sent row
    /// vanishes, the consumed-draft tombstone lifts, and the send's content is
    /// re-queued as a draft save (undo restores the full compose durably). The
    /// echoes converge the client without a sync.
    ///
    /// @spec docs/L1-outbox#state-machine
    pub async fn discard_operation(
        &self,
        operation_id: &OperationId,
    ) -> Result<Option<Vec<DomainEvent>>, ServiceError> {
        // Snapshot BEFORE the guarded removal (for the re-derivation targets);
        // the removal itself remains the single racing statement.
        let snapshot = self.outbox.get_operation(operation_id)?;
        if self.outbox.remove_operation_unless_inflight(operation_id)? {
            let mut events = Vec::new();
            if let Some(operation) = snapshot {
                if operation.kind == OperationKind::Send {
                    events = self.unwind_send_fold(&operation).await?;
                    // Undo restores the FULL compose durably: the send's
                    // content (the newest edit, last-writer-wins) is re-queued
                    // as an ordinary draft save under its compose key —
                    // resumable offline, ensured provider-side on the next
                    // flush. No client persist-then-schedule needed.
                    if let Ok(posthaste_domain_model::MailIntent::Send(request)) =
                        operation.intent()
                    {
                        if let Some(key) = request
                            .draft_id
                            .as_deref()
                            .map(str::trim)
                            .filter(|key| !key.is_empty())
                        {
                            let (_save, save_events) = self
                                .save_draft(
                                    &operation.account_id,
                                    Some(MessageId::from(key)),
                                    request.clone(),
                                )
                                .await?;
                            events.extend(save_events);
                        }
                    }
                } else if operation.kind.is_draft_save() {
                    // A cancelled draft save leaves NO phantom: its derived row
                    // is gone the moment the op is removed. Re-derive the live
                    // row (it retires — base alone remains, i.e. nothing) and
                    // prune it client-side if it was showing.
                    let live = self
                        .draft_registry
                        .resolve_draft_entity(&operation.account_id, &operation.entity.id)?
                        .unwrap_or_else(|| operation.entity.id.clone());
                    let live_id = MessageId::from(live.as_str());
                    // Atomic retire echo: the in-txn diff is the retire signal —
                    // no before/after reads (closes the TOCTOU a racing base
                    // write could flip between them).
                    let diff = self
                        .refresh_message_overlay(&operation.account_id, &live_id)
                        .await?;
                    if diff.effectively_retired() {
                        events.push(self.events.append_event(
                            &operation.account_id,
                            EVENT_TOPIC_MESSAGE_UPDATED,
                            None,
                            Some(&live_id),
                            serde_json::json!({ "messageId": live_id.as_str(), "deleted": true }),
                        )?);
                    }
                }
            }
            return Ok(Some(events));
        }
        // Nothing removed: the op never existed, it is in flight and must not
        // be yanked, or it is settled (`applied`) — the provider already
        // accepted it, so it rests in the log until causal truncation and the
        // cancel observes it as already gone (`None`), exactly as if
        // settlement had removed it.
        match self.outbox.get_operation(operation_id)? {
            Some(operation) if operation.state == OperationState::Inflight => Err(
                GatewayError::Rejected("cannot discard an in-flight operation".to_string()).into(),
            ),
            _ => Ok(None),
        }
    }

    /// Unwind a discarded send's folded effects. Discard removed the send op,
    /// so its derived rows are gone the moment replay runs — zero unwind
    /// bookkeeping: re-derive the provisional Sent row (its entry retires, base
    /// alone remains — nothing) and the consumed draft's live row (the send's
    /// tombstone fold is gone, so the draft's own save op — if any — re-derives
    /// it, or it retires), echoing both so the client converges without a sync.
    /// A HELD send folded nothing, so both refreshes are no-ops for the common
    /// undo. The caller re-queues the send's content as a fresh draft save,
    /// which then derives the recovery draft row.
    pub(super) async fn unwind_send_fold(
        &self,
        operation: &Operation,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let account_id = &operation.account_id;
        let mut events = Vec::new();
        let send_row_id = MessageId::from(operation.entity.id.as_str());
        // The provisional Sent row is derived from the (now-removed) send op:
        // re-derive so it retires, and prune it client-side if it was showing.
        // The in-txn diff is the retire signal — no before/after reads.
        let diff = self
            .refresh_message_overlay(account_id, &send_row_id)
            .await?;
        if diff.effectively_retired() {
            events.push(self.events.append_event(
                account_id,
                EVENT_TOPIC_MESSAGE_UPDATED,
                None,
                Some(&send_row_id),
                serde_json::json!({ "messageId": send_row_id.as_str(), "deleted": true }),
            )?);
        }
        if let Ok(posthaste_domain_model::MailIntent::Send(request)) = operation.intent() {
            if let Some(key) = request
                .draft_id
                .as_deref()
                .map(str::trim)
                .filter(|key| !key.is_empty())
            {
                let live = self
                    .draft_registry
                    .resolve_draft_entity(account_id, key)?
                    .unwrap_or_else(|| key.to_string());
                let live_id = MessageId::from(live.as_str());
                self.refresh_message_overlay(account_id, &live_id).await?;
                if let Some(summary) = self
                    .message_detail_reader
                    .get_message_summary(account_id, &live_id)?
                {
                    let scope = summary.mailbox_ids.first().cloned();
                    events.push(self.events.append_event(
                        account_id,
                        EVENT_TOPIC_MESSAGE_UPDATED,
                        scope.as_ref(),
                        Some(&live_id),
                        serde_json::json!({
                            "messageId": live_id.as_str(),
                            "changes": { "mailboxes": true },
                            "projection": &summary,
                        }),
                    )?);
                }
            }
        }
        Ok(events)
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

    /// Construct and enqueue an operation, capturing creation timestamps.
    /// There is no cross-operation dependency edge (D174): state assertions
    /// coalesce here, draft saves coalesce in
    /// [`Self::save_draft`](crate::service::MailService::save_draft), and
    /// everything else relies on the flusher's insertion-order drain.
    ///
    /// @spec docs/L1-outbox#operation-model
    pub fn queue_operation(
        &self,
        account_id: &AccountId,
        entity: OperationEntity,
        kind: OperationKind,
        payload: serde_json::Value,
        send_at: Option<String>,
        hold_until_mono: Option<i64>,
    ) -> Result<Operation, ServiceError> {
        self.queue_operation_with_id(
            None,
            account_id,
            entity,
            kind,
            payload,
            send_at,
            hold_until_mono,
        )
    }

    /// [`Self::queue_operation`] with a caller-supplied operation id, making
    /// that id the durable idempotency key: enqueueing is idempotent on it,
    /// so replaying the same id returns the already-stored operation instead
    /// of inserting a second one. `None` mints a fresh id.
    #[allow(clippy::too_many_arguments)]
    pub fn queue_operation_with_id(
        &self,
        operation_id: Option<OperationId>,
        account_id: &AccountId,
        entity: OperationEntity,
        kind: OperationKind,
        mut payload: serde_json::Value,
        send_at: Option<String>,
        hold_until_mono: Option<i64>,
    ) -> Result<Operation, ServiceError> {
        if kind.is_state_assertion() {
            // State assertions coalesce instead of chaining: a new assertion
            // supersedes (or merges with) the pending assertion it replaces, so
            // the outbox holds the latest desired state per (entity, kind).
            self.coalesce_pending_assertions(account_id, &entity, kind, &mut payload)?;
        }
        let now =
            now_iso8601().map_err(|error| ServiceError::from(GatewayError::Rejected(error)))?;
        let operation = Operation {
            id: operation_id.unwrap_or_else(|| OperationId::from(Id::generate().to_string())),
            account_id: account_id.clone(),
            entity,
            kind,
            payload,
            payload_version: 1,
            state: OperationState::Pending,
            attempts: 0,
            last_error: None,
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

    /// Enqueue an outgoing message local-first, materializing what the send
    /// MEANS at admission (D170), folding its effects into the overlay plane,
    /// and returning the projection echoes.
    ///
    /// The send is queued and flushed to the provider on the next connectivity
    /// window; the caller does not need a live gateway. A unique entity id makes
    /// the operation its own idempotency unit so it never coalesces and is sent
    /// at most once (see the send-once recovery in [`Self::flush_account`]).
    ///
    /// MATERIALIZATION (D170): the client's `draftId` is an unresolved
    /// compose-key gesture. Admission consults the one identity authority —
    /// the draft registry (+ the projection for headerless resumed drafts) —
    /// and stamps the decision: a key that names a known draft makes this a
    /// CONSUMING send (the fold tombstones the draft's live row when due; the
    /// flush destroys the provider copy in the send's own execution); an
    /// unknown key is dropped (a plain send). The client's stale view is
    /// never load-bearing.
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
    pub async fn enqueue_send(
        &self,
        account_id: &AccountId,
        request: SendMessageRequest,
    ) -> Result<(Operation, Vec<DomainEvent>), ServiceError> {
        self.enqueue_send_with_operation_id(account_id, request, None)
            .await
    }

    /// [`Self::enqueue_send`] with a caller-supplied outbox operation id, so
    /// an API-level intent id becomes the durable idempotency key for the
    /// send: replaying the same id returns the already-queued operation
    /// instead of enqueuing — and eventually dispatching — a second one.
    pub async fn enqueue_send_with_operation_id(
        &self,
        account_id: &AccountId,
        mut request: SendMessageRequest,
        operation_id: Option<OperationId>,
    ) -> Result<(Operation, Vec<DomainEvent>), ServiceError> {
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
        // D170 materialization: resolve the compose key against the registry
        // (+ projection). Known → a consuming send, and the key is RESERVED
        // (self-mapped) if the registry does not hold it yet, so the flush
        // resolve is total; unknown → a plain send, key dropped.
        let compose_key = request
            .draft_id
            .take()
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty());
        let held = send_at.is_some() || hold_until_mono.is_some();
        let consumes_key = match compose_key {
            Some(key) => {
                let registered = self
                    .draft_registry
                    .resolve_draft_entity(account_id, &key)?
                    .is_some();
                if registered || self.draft_message_exists(account_id, &key)? {
                    if !registered {
                        self.draft_registry
                            .set_draft_alias(account_id, &key, &key)?;
                    }
                    Some(key)
                } else if held {
                    // D173: a HELD send is ALWAYS consuming — its two-step
                    // plan's eager ensure-draft creates the provider copy
                    // (cross-device visibility during the hold), so an
                    // unknown key is reserved rather than dropped.
                    self.draft_registry
                        .set_draft_alias(account_id, &key, &key)?;
                    Some(key)
                } else {
                    None
                }
            }
            None if held => {
                // A key-less held send mints its compose key at admission —
                // the ensure-draft step needs a draft identity to create,
                // and the submit step consumes it.
                let key = format!("draft-local-{}", Id::generate());
                self.draft_registry
                    .set_draft_alias(account_id, &key, &key)?;
                Some(key)
            }
            None => None,
        };
        request.draft_id = consumes_key.clone();
        // The normalized hold lives on the OPERATION (the flush gate reads the
        // indexed column); it is dropped from the payload so an immediate
        // send's payload — and the bytes the gateway sees — stay identical to
        // the pre-feature shape.
        let payload = encode_payload(request, "send request")?;
        let operation = self.queue_operation_with_id(
            operation_id,
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Message,
                id: format!(
                    "{}{}",
                    posthaste_domain_model::SEND_ENTITY_ID_PREFIX,
                    Id::generate()
                ),
            },
            OperationKind::Send,
            payload,
            send_at,
            hold_until_mono,
        )?;
        // Fold the send's effects (D172, phase-aware: a held send folds
        // nothing — the draft stays visible and cancelable; a due send
        // tombstones the consumed draft and upserts the provisional Sent
        // row) and echo the changed rows from the effective read.
        let mut events = Vec::new();
        let send_row_id = MessageId::from(operation.entity.id.as_str());
        let consumed_live = match &consumes_key {
            Some(key) => Some(MessageId::from(
                self.draft_registry
                    .resolve_draft_entity(account_id, key)?
                    .unwrap_or_else(|| key.clone())
                    .as_str(),
            )),
            None => None,
        };
        self.refresh_message_overlay(account_id, &send_row_id)
            .await?;
        if let Some(live_id) = &consumed_live {
            // In-txn retire signal — no before/after reads (closes the TOCTOU).
            // The projection branch still reads the effective summary for its
            // content; the overlay-plane derive can't produce the joined
            // MessageSummary (account/conversation/version fields it lacks).
            let diff = self.refresh_message_overlay(account_id, live_id).await?;
            if diff.effectively_retired() {
                // The fold hid a visible row (a due consuming send): prune it.
                events.push(self.events.append_event(
                    account_id,
                    EVENT_TOPIC_MESSAGE_UPDATED,
                    None,
                    Some(live_id),
                    serde_json::json!({ "messageId": live_id.as_str(), "deleted": true }),
                )?);
            } else if let Some(summary) = self
                .message_detail_reader
                .get_message_summary(account_id, live_id)?
            {
                // A held send's draft-form row (appeared or refreshed):
                // project it so the hold is visible without any client save.
                let scope = summary.mailbox_ids.first().cloned();
                events.push(self.events.append_event(
                    account_id,
                    EVENT_TOPIC_MESSAGE_UPDATED,
                    scope.as_ref(),
                    Some(live_id),
                    serde_json::json!({
                        "messageId": live_id.as_str(),
                        "changes": { "mailboxes": true },
                        "projection": &summary,
                    }),
                )?);
            }
        }
        if let Some(summary) = self
            .message_detail_reader
            .get_message_summary(account_id, &send_row_id)?
        {
            let scope = summary.mailbox_ids.first().cloned();
            events.push(self.events.append_event(
                account_id,
                EVENT_TOPIC_MESSAGE_UPDATED,
                scope.as_ref(),
                Some(&send_row_id),
                serde_json::json!({
                    "messageId": send_row_id.as_str(),
                    "changes": { "mailboxes": true },
                    "projection": &summary,
                }),
            )?);
        }
        Ok((operation, events))
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
