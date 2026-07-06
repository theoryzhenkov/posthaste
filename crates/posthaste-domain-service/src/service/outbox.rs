//! Tier-2 (runtime <-> provider) outbox engine: enqueue and flush.
//!
//! Callers enqueue an [`Operation`]; pending operations form a read-time overlay
//! and the flusher drains them to the provider, settling applied/failed outcomes
//! and emitting `operation.settled` events. Draft ops carry the STABLE draft key
//! as their entity id and resolve it to the current live id at push time via the
//! `DraftRegistry` (M70/D136); a provider-assigned rotation is recorded as one
//! registry repoint at settlement.
//!
//! @spec docs/L1-outbox#operation-model
//! @spec docs/L1-outbox#state-machine

use super::message_queries::project_record;
use super::*;
use posthaste_domain_model::{
    CommandAck, MessageReadback, MessageRecord, MutationOutcome, OperationDispatchUncertain,
    SyncBatch, EVENT_TOPIC_OPERATION_DISPATCH_UNCERTAIN,
};

/// How a push failure routes the operation. A superset of the D70 retryability
/// verdict ([`Terminality`]): the send path adds a third, non-blind-retryable
/// disposition (`Uncertain`) for a possibly-delivered send.
enum FlushDisposition {
    /// The network recovers on its own — keep the op pending, stop draining
    /// (offline), retry next window.
    Transient,
    /// Retrying the same push cannot change the outcome — fail and surface.
    Permanent,
    /// A **send** whose delivery outcome is unknown (timeout/transport-loss
    /// after the submission may have committed). Park in `DispatchUncertain`
    /// (RFC-L2 D86) — never blind-resent; only an explicit user retry (under the
    /// same idempotency identity) or a discard resolves it.
    Uncertain,
}

/// Outcome of attempting to push one operation to the provider: the routing
/// [`FlushDisposition`] plus the human-readable message recorded on the
/// operation / surfaced in the settlement. The verdict is data, not a string
/// bucket.
struct FlushError {
    disposition: FlushDisposition,
    message: String,
}

impl FlushError {
    /// A local, provider-independent permanent failure (e.g. an un-decodable
    /// stored payload) — there is no `GatewayError` to classify.
    fn permanent(message: impl Into<String>) -> Self {
        Self {
            disposition: FlushDisposition::Permanent,
            message: message.into(),
        }
    }
}

enum DependencyStatus {
    Satisfied,
    Waiting,
    Cancelled(String),
}

/// Outcome of one [`MailService::flush_pass`] over the flushable operations.
struct FlushPass {
    /// The drain stopped early on a transient/uncertain failure — the caller
    /// must not re-pass; the rest retries on the next connectivity window.
    stopped: bool,
    /// A settlement effect enqueued a follow-up operation this pass (a settled
    /// send consumed its draft — D126), so the caller drains once more.
    follow_up_enqueued: bool,
}

/// Classify a gateway failure into its typed retryability verdict plus the
/// message recorded on the operation. Exhaustive over [`GatewayError`] by design
/// (the M29 gate): a new variant fails to compile here until its terminality is
/// decided — no `other => Permanent(to_string())` free-text catch-all.
fn classify_gateway_error(error: GatewayError) -> FlushError {
    let (disposition, message) = match error {
        // Reachable-again: the network recovers on its own.
        GatewayError::Network(message) | GatewayError::Unavailable(message) => {
            (FlushDisposition::Transient, message)
        }
        // Auth failures are transient: they clear once the account re-authenticates.
        GatewayError::Auth => (
            FlushDisposition::Transient,
            "authentication required".to_string(),
        ),
        // A send whose outcome is unknown — the request may have committed after
        // the transport dropped. Never blind-resend: park it (D86).
        GatewayError::DispatchUncertain(message) => (FlushDisposition::Uncertain, message),
        // Terminal as written — a diverged state, a provider rejection, a corrupt
        // local store, or an internal codec bug — retrying the same push cannot
        // change the outcome.
        GatewayError::StateMismatch => (
            FlushDisposition::Permanent,
            "provider state diverged".to_string(),
        ),
        GatewayError::CannotCalculateChanges => (
            FlushDisposition::Permanent,
            "cannot calculate changes".to_string(),
        ),
        GatewayError::Rejected(message)
        | GatewayError::Corruption(message)
        | GatewayError::Internal(message) => (FlushDisposition::Permanent, message),
        GatewayError::MutationRejected { reason, .. } => (FlushDisposition::Permanent, reason),
        // Mailbox destroy is a synchronous mutation (never queued in the outbox),
        // so this refusal cannot actually reach the flush path; classify it
        // permanent for exhaustiveness — retrying the same push can't change it.
        GatewayError::MailboxNotEmpty { count } => (
            FlushDisposition::Permanent,
            format!("mailbox is not empty ({count} messages)"),
        ),
    };
    FlushError {
        disposition,
        message,
    }
}

/// Result of pushing one operation to the provider.
#[allow(clippy::large_enum_variant)]
enum Pushed {
    /// A non-message entity op (draft/send): settle and remove.
    /// `assigned_entity_id` is the provider id a draft save returned — the
    /// settlement repoints the op's stable draft key to it in the registry
    /// (M70/D136: the op's entity id IS the key and never rotates).
    /// `destroyed_entity_id` is the live id a draft destroy resolved to at
    /// flush time, so the settlement's reconciling event names the projected
    /// row rather than the stable key the op carries.
    Entity {
        assigned_entity_id: Option<String>,
        destroyed_entity_id: Option<String>,
    },
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
        // `draft_key` is a stable, client-chosen handle for the draft. The
        // registry maps it to the entity id currently representing the draft (a
        // temporary id before the first flush, a provider id after), so the
        // client can keep using the same key across flushes without creating
        // duplicate drafts.
        let key = draft_key
            .map(|id| id.to_string())
            .unwrap_or_else(|| format!("draft-local-{}", Id::generate()));
        // Stamp the stable identity into the payload so the gateway writes it as
        // the `X-Posthaste-Draft-Id` header. The header survives the provider id
        // rotation a JMAP draft update causes and is read back on resume, so the
        // client always keys by a stable value.
        request.draft_id = Some(key.clone());
        // M70 (D136): the op carries the STABLE key as its entity id — the live
        // id is resolved at flush, immediately before the gateway call, so the
        // push always targets the freshest mapping the registry knows (a
        // rotation observed between enqueue and flush cannot stale it).
        // Enqueue-time resolution only picks the kind: a key the registry knows
        // is an update; an unknown key registers itself (a self-mapping until
        // the first flush assigns a provider id).
        let kind = match self.draft_registry.resolve_draft_entity(account_id, &key)? {
            Some(_) => OperationKind::DraftUpdate,
            None => {
                self.draft_registry
                    .set_draft_alias(account_id, &key, &key)?;
                // A key with no registry row that already names an existing
                // draft message is a draft resumed by its (rotating) provider
                // id — a legacy draft saved before stable ids, or one created
                // elsewhere. Replace it in place instead of creating a
                // duplicate; this also bootstraps a stable header onto it.
                if self.draft_message_exists(account_id, &key)? {
                    OperationKind::DraftUpdate
                } else {
                    OperationKind::DraftCreate
                }
            }
        };
        let payload = encode_payload(request, "draft request")?;
        self.queue_operation(
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Draft,
                id: key,
            },
            kind,
            payload,
        )
    }

    /// Delete a draft local-first: enqueue a draft delete operation carrying
    /// the draft's STABLE key (M70/D136). The live entity id (temporary or
    /// provider-assigned) is resolved at flush, immediately before the provider
    /// destroy, so a rotation observed between enqueue and flush (an in-flight
    /// save settling, a sync-observed other-device edit repointing the
    /// registry) retargets the destroy to the current live draft.
    ///
    /// The registry mapping is NOT forgotten here (M70): identity survives
    /// until the destruction is confirmed — at this op's settlement (the
    /// `DraftDelete` Applied arm of [`Self::flush_pass`]) or at sync-observed
    /// disappearance (M69's confirmed-gone prune) — so an in-flight op never
    /// references a forgotten mapping.
    ///
    /// `idempotent_redelivery` records whether a provider `notFound` at flush
    /// time is a benign already-gone (the send-consume settlement effect, which
    /// re-enqueues the delete under redelivery — D126) or a genuine failure a
    /// user-initiated discard must surface (D133). It is stamped onto the op so
    /// the gateway narrows its `notFound ⇒ Ok` mask to the idempotent case only.
    ///
    /// @spec docs/L1-outbox#operation-model
    /// @spec docs/eph/RFC-L2-draft-identity#22-d136--one-seam-the-draftregistry-port-resolve-at-flush
    pub fn delete_draft(
        &self,
        account_id: &AccountId,
        draft_key: MessageId,
        idempotent_redelivery: bool,
    ) -> Result<Operation, ServiceError> {
        self.queue_operation(
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Draft,
                id: draft_key.to_string(),
            },
            OperationKind::DraftDelete,
            serde_json::json!({ "idempotentRedelivery": idempotent_redelivery }),
        )
    }

    /// Discard a draft through the optimistic runtime-mutation path (D130).
    ///
    /// Unlike [`delete_draft`] (the send-consume settlement effect), this is
    /// user-initiated: it resolves the stable draft key to the live entity
    /// (D131), removes the local draft row and emits the reconciling
    /// `message.updated{deleted:true}` immediately so the client's base prunes
    /// and the optimistic destroy retires without a follow-up sync (D132), and
    /// queues the provider destroy as a NON-idempotent op (D133 — a `notFound`
    /// now surfaces retryably). A key that no longer names a live draft is a
    /// surfaced `NotFound`, not a silent success: the client reverts the
    /// optimistic fold and shows the error (D133/D134).
    ///
    /// @spec docs/eph/RFC-L2-drafts#rfc-part-2
    pub async fn discard_draft(
        &self,
        account_id: &AccountId,
        draft_key: MessageId,
    ) -> Result<CommandAck, ServiceError> {
        let key = draft_key.to_string();
        let entity_id = self
            .draft_registry
            .resolve_draft_entity(account_id, &key)?
            .unwrap_or_else(|| key.clone());
        let message_id = MessageId::from(entity_id.as_str());
        // A discard of a draft that no longer resolves to a live local row must
        // surface (D133) — the optimistic fold on the client reverts + shows the
        // error rather than silently "succeeding".
        let mailboxes = self
            .message_mailboxes
            .get_message_mailboxes(account_id, &message_id)?;
        if mailboxes.is_empty()
            && self
                .message_detail_reader
                .get_message_summary(account_id, &message_id)?
                .is_none()
        {
            return Err(ServiceError::from(StoreError::NotFound(format!(
                "draft:{}",
                message_id.as_str()
            ))));
        }
        // Queue the provider destroy (non-idempotent) via the stable-id path.
        let operation = self.delete_draft(account_id, draft_key, false)?;
        // Optimistic local removal (write-through) so canonical reflects the
        // discard immediately, mirroring `destroy_message`. On a local failure,
        // retract the op so the outbox and canonical do not diverge.
        let message_commands = self.message_commands.clone();
        let owned_account = account_id.clone();
        let owned_message = message_id.clone();
        if let Err(error) =
            offload(move || message_commands.destroy_message(&owned_account, &owned_message, None))
                .await
        {
            let _ = self.outbox.remove_operation(&operation.id);
            return Err(ServiceError::from(error));
        }
        let event = match self.events.append_event(
            account_id,
            EVENT_TOPIC_MESSAGE_UPDATED,
            mailboxes.first(),
            Some(&message_id),
            serde_json::json!({ "messageId": message_id.as_str(), "deleted": true }),
        ) {
            Ok(event) => event,
            Err(error) => {
                let _ = self.outbox.remove_operation(&operation.id);
                return Err(ServiceError::from(error));
            }
        };
        Ok(CommandAck {
            events: vec![event],
        })
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
    /// A settlement effect can enqueue a follow-up operation mid-pass (a settled
    /// send consumes its draft — D126); the outer loop re-lists and drains those
    /// follow-ups in the same call so the draft leaves the provider before this
    /// flush's caller (the sync cycle) pulls. Bounded: it re-passes only when
    /// this pass enqueued a follow-up, and follow-ups (draft deletes) enqueue
    /// nothing themselves.
    ///
    /// @spec docs/L1-outbox#state-machine
    /// @spec docs/eph/RFC-L2-drafts#3-decisions-proposed
    pub async fn flush_account(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let mut events = Vec::new();
        loop {
            let pass = self.flush_pass(account_id, gateway, &mut events).await?;
            if pass.stopped || !pass.follow_up_enqueued {
                break;
            }
        }
        Ok(events)
    }

    /// One drain pass over the currently-flushable operations. Appends the
    /// settlement events to `events` and reports whether the drain stopped
    /// early (transient/uncertain) and whether a settlement effect enqueued a
    /// follow-up operation ([`Self::flush_account`] re-passes on the latter).
    async fn flush_pass(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
        events: &mut Vec<DomainEvent>,
    ) -> Result<FlushPass, ServiceError> {
        let queued = self.outbox.list_flushable_operations(account_id)?;
        let mut follow_up_enqueued = false;
        for snapshot in queued {
            // Re-fetch fresh: an earlier op in this pass may have changed this
            // op's state (draft entity ids no longer rotate — M70: draft ops
            // carry the stable key and resolve it at their own push).
            let Some(operation) = self.outbox.get_operation(&snapshot.id)? else {
                continue;
            };
            if !operation.state.is_flushable() {
                continue;
            }
            // Send-once: a `send` found already `inflight` was interrupted after
            // a prior flush began, so the message may already have left the
            // provider. Never blind-resend it; park it as dispatch-uncertain and
            // surface it for the user to confirm or discard (RFC-L2 D86 —
            // generalizes the crashed-inflight guard from "crash" to
            // "uncertainty"; the timeout path below reaches the same state).
            //
            // @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
            if operation.kind == OperationKind::Send && operation.state == OperationState::Inflight
            {
                let reason = "send interrupted mid-flush; delivery uncertain".to_string();
                self.outbox.update_operation_state(
                    &operation.id,
                    OperationState::DispatchUncertain,
                    operation.attempts,
                    Some(&reason),
                )?;
                events.push(self.emit_dispatch_uncertain(account_id, &operation, reason)?);
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
                Ok(Pushed::Entity {
                    assigned_entity_id,
                    destroyed_entity_id,
                }) => {
                    if let Some(new_id) = assigned_entity_id.as_deref() {
                        if new_id != operation.entity.id {
                            // M70 (D136): a draft op's entity id IS the stable
                            // key and never rotates, so a provider rotation is
                            // recorded as ONE registry write — key → live id.
                            // Later ops carry the key too and resolve it fresh
                            // at their own flush; no outbox rewrite is needed.
                            self.draft_registry.set_draft_alias(
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
                    if operation.kind == OperationKind::DraftDelete {
                        // M70: forget at SETTLEMENT — the provider has confirmed
                        // the destroy, so only now does the stable key stop
                        // naming a live draft (never at enqueue: an in-flight op
                        // must still resolve its mapping). Converges with M69's
                        // sync-observed forget (the confirmed-gone prune): both
                        // are idempotent deletes of the same registry row, so
                        // whichever observes the confirmed destruction second is
                        // a no-op — the mapping is forgotten exactly once, on
                        // confirmed destruction.
                        self.draft_registry
                            .remove_draft_alias(account_id, &operation.entity.id)?;
                        // D132: a settled DraftDelete emits the reconciling
                        // `message.updated{deleted:true}` so the client's
                        // fold/prune converges without leaning on a follow-up
                        // sync (the send-consume path has no apply-time event;
                        // the user-discard path already emitted one — this is an
                        // idempotent backstop). It names the LIVE id the destroy
                        // resolved to at flush, not the stable key, so it prunes
                        // the projected row.
                        let deleted_id = destroyed_entity_id
                            .as_deref()
                            .unwrap_or(operation.entity.id.as_str());
                        events.push(self.events.append_event(
                            account_id,
                            EVENT_TOPIC_MESSAGE_UPDATED,
                            None,
                            Some(&MessageId::from(deleted_id)),
                            serde_json::json!({
                                "messageId": deleted_id,
                                "deleted": true,
                            }),
                        )?);
                    }
                    self.outbox.remove_operation(&operation.id)?;
                    // D126: a settled send consumes its originating draft — the
                    // destroy is enqueued as a follow-up op so it is retried
                    // with the outbox discipline, never silently dropped.
                    if self
                        .consume_draft_after_send(account_id, &operation)?
                        .is_some()
                    {
                        follow_up_enqueued = true;
                    }
                }
                Ok(Pushed::Message { readback, rejected }) => {
                    // Settle now from the provider readback: remove the op, fold
                    // the remaining unsettled assertions over the readback, and
                    // write canonical — the settle write reverts a rejected change
                    // (its readback is the unchanged row) and emits the recompute.
                    //
                    // @spec docs/eph/DESIGN-L2-optimistic-projection#3-the-runtime-write-through-mechanics
                    events.extend(
                        self.settle_message_operation(account_id, &operation, readback, rejected)
                            .await?,
                    );
                }
                Err(FlushError {
                    disposition: FlushDisposition::Transient,
                    message,
                }) => {
                    // Re-queue Pending WITHOUT reconciling the entity id: a
                    // DraftCreate/DraftUpdate whose create+destroy committed but
                    // whose response was lost (e.g. GatewayError::Network) lands
                    // here with its entity id un-reconciled. That is safe now (DS2):
                    // the create-id is derived from `operation.id` — preserved
                    // across this re-queue (only the state + attempts change, never
                    // the op id) — so the retry re-issues the SAME create-id and the
                    // server no-ops the duplicate create instead of orphaning a twin
                    // draft. Rotating the op id here would break that idempotency.
                    self.outbox.update_operation_state(
                        &operation.id,
                        OperationState::Pending,
                        operation.attempts + 1,
                        Some(&message),
                    )?;
                    // Offline: stop draining; the rest retries next window.
                    return Ok(FlushPass {
                        stopped: true,
                        follow_up_enqueued,
                    });
                }
                Err(FlushError {
                    disposition: FlushDisposition::Uncertain,
                    message,
                }) => {
                    // A send whose delivery is unknown: park it, never resend
                    // (D86). Removed from the flush set until the user acts.
                    self.outbox.update_operation_state(
                        &operation.id,
                        OperationState::DispatchUncertain,
                        operation.attempts + 1,
                        Some(&message),
                    )?;
                    events.push(self.emit_dispatch_uncertain(account_id, &operation, message)?);
                    // A send timeout signals a struggling link; stop draining so
                    // the rest retries on the next connectivity window.
                    return Ok(FlushPass {
                        stopped: true,
                        follow_up_enqueued,
                    });
                }
                Err(FlushError {
                    disposition: FlushDisposition::Permanent,
                    message,
                }) => {
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
        Ok(FlushPass {
            stopped: false,
            follow_up_enqueued,
        })
    }

    /// D126: draft destruction is a settlement effect of the send. When a
    /// settled-successful `Send` carries the originating draft's stable id
    /// (`SendMessageRequest::draft_id`), enqueue the draft's delete so the
    /// consumed draft leaves the provider's Drafts mailbox. Enqueued — not
    /// pushed inline — so a transient destroy failure is retried with the
    /// outbox/settlement machinery, never silent, and never re-runs the send.
    ///
    /// Idempotent across settlement redelivery (ruling 24): once the consumed
    /// draft's destroy settles, its registry mapping is forgotten (M70 —
    /// settlement-time forget), and the gateways treat an already-gone draft as
    /// destroyed, so a redelivered send settlement enqueues nothing (unknown
    /// draft) or settles an at-worst harmless second delete.
    ///
    /// On a parked send (`DispatchUncertain`) this is never reached — the draft
    /// is KEPT as the user's recovery artifact (D125); destruction happens only
    /// on settled success.
    ///
    /// @spec docs/eph/RFC-L2-drafts#3-decisions-proposed
    fn consume_draft_after_send(
        &self,
        account_id: &AccountId,
        operation: &Operation,
    ) -> Result<Option<Operation>, ServiceError> {
        if operation.kind != OperationKind::Send {
            return Ok(None);
        }
        // The payload decoded to push the send, so a failure here is
        // unreachable in practice; it must not un-settle the settled send.
        let Ok(request) = serde_json::from_value::<SendMessageRequest>(operation.payload.clone())
        else {
            return Ok(None);
        };
        let Some(key) = request
            .draft_id
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
        else {
            return Ok(None);
        };
        // A key that resolves to no alias and no projected message names a
        // draft already consumed (redelivery) or never saved — nothing to do.
        let known = self
            .draft_registry
            .resolve_draft_entity(account_id, key)?
            .is_some()
            || self.draft_message_exists(account_id, key)?;
        if !known {
            return Ok(None);
        }
        self.delete_draft(account_id, MessageId::from(key), true)
            .map(Some)
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

    /// M70 (D136): resolve a draft op's stable key to the live entity id at
    /// flush time — immediately before the gateway call — so the push targets
    /// the freshest mapping the registry knows. This closes the in-flight-op
    /// vs sync race M69 flagged: a sync chunk that repointed the registry (a
    /// rotation observed from another device) between enqueue and flush is
    /// reflected in the target. A key the registry no longer knows falls back
    /// to itself — the pre-M70 enqueue-time semantics, preserved so the
    /// provider still surfaces `notFound` per D133 (the uniform already-gone
    /// reading is M71/D137, for which this freshness guarantee is the
    /// prerequisite).
    ///
    /// @spec docs/eph/RFC-L2-draft-identity#22-d136--one-seam-the-draftregistry-port-resolve-at-flush
    fn resolve_draft_flush_target(
        &self,
        account_id: &AccountId,
        draft_key: &str,
    ) -> Result<String, FlushError> {
        Ok(self
            .draft_registry
            .resolve_draft_entity(account_id, draft_key)
            .map_err(|error| {
                FlushError::permanent(format!("draft identity resolution failed: {error}"))
            })?
            .unwrap_or_else(|| draft_key.to_string()))
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
                // A create has no replace target, so the DS3 redelivery flag is
                // irrelevant (no destroy outcome to mask). The operation id is the
                // stable create identity (constant across retries): the gateway
                // derives a deterministic `Email/set` create-id from it (DS2), so a
                // lost-response redelivery re-creates under the same id and cannot
                // orphan a twin draft.
                let new_id = gateway
                    .save_draft(account_id, &request, None, false, operation.id.as_str())
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: Some(new_id.to_string()),
                    destroyed_entity_id: None,
                })
            }
            OperationKind::DraftUpdate => {
                let request = parse_payload::<SendMessageRequest>(operation)?;
                // M70 (D136): the op carries the stable draft key; resolve it
                // to the CURRENT live id here, immediately before the gateway
                // call, so the replace targets the freshest mapping (in-session
                // rotations and sync-observed ones alike).
                let replace_id =
                    self.resolve_draft_flush_target(account_id, &operation.entity.id)?;
                let replace = MessageId::from(replace_id.as_str());
                // DS3/D133: a re-flush of this save (attempts > 0) may have already
                // committed the prior-draft destroy on an earlier attempt, so an
                // already-gone replace target is benign; a first delivery's failed
                // replace-destroy surfaces so the save is retried rather than
                // silently leaving the old draft behind (the twin).
                let idempotent_redelivery = operation.attempts > 0;
                // The operation id is the stable create identity (constant across
                // retries): the gateway derives a deterministic `Email/set`
                // create-id from it (DS2), so a redelivery whose create+destroy
                // committed but whose response was lost re-creates under the same
                // id and cannot orphan a twin draft.
                let new_id = gateway
                    .save_draft(
                        account_id,
                        &request,
                        Some(&replace),
                        idempotent_redelivery,
                        operation.id.as_str(),
                    )
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: Some(new_id.to_string()),
                    destroyed_entity_id: None,
                })
            }
            OperationKind::DraftDelete => {
                // M70 (D136): resolve the stable key to the live destroy target
                // at flush (see [`Self::resolve_draft_flush_target`]) — a
                // registry repoint between enqueue and flush retargets the
                // destroy to the draft's current live id.
                let target_id =
                    self.resolve_draft_flush_target(account_id, &operation.entity.id)?;
                let target = MessageId::from(target_id.as_str());
                // D133: only an idempotent redelivery (a send-consume re-enqueue)
                // masks a provider `notFound` as success; a user discard's
                // `notFound` surfaces as a retryable failure.
                let idempotent_redelivery = operation
                    .payload
                    .get("idempotentRedelivery")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                gateway
                    .delete_draft(account_id, &target, idempotent_redelivery)
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: None,
                    destroyed_entity_id: Some(target_id),
                })
            }
            OperationKind::Send => {
                let request = parse_payload::<SendMessageRequest>(operation)?;
                // The operation id is the send's stable idempotency identity
                // (constant across retries): the gateway derives the JMAP
                // EmailSubmission create-id + `ifInState` and the SMTP/JMAP
                // Message-ID from it (D84/D85), so a re-forward of a send that
                // already committed is deduplicated, not duplicated.
                gateway
                    .send_message(account_id, &request, operation.id.as_str())
                    .await
                    .map_err(classify_gateway_error)?;
                Ok(Pushed::Entity {
                    assigned_entity_id: None,
                    destroyed_entity_id: None,
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
    async fn settle_message_operation(
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
            let sync_writer = self.sync_writer.clone();
            let owned_account_id = account_id.clone();
            events.extend(
                offload(move || sync_writer.apply_sync_batch(&owned_account_id, &batch)).await?,
            );
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

    /// Emit `operation.dispatch_uncertain` for a parked send. Carries the op id
    /// and the uncertainty reason so downstream tiers (web notification center,
    /// the tap/scripting surface) can raise a needs-attention signal — a parked
    /// send with no surface is data loss with extra steps (RFC-L2 §7/O1).
    ///
    /// @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
    fn emit_dispatch_uncertain(
        &self,
        account_id: &AccountId,
        operation: &Operation,
        reason: String,
    ) -> Result<DomainEvent, ServiceError> {
        let message_id = MessageId::from(operation.entity.id.as_str());
        let payload = encode_payload(
            &OperationDispatchUncertain {
                id: operation.id.clone(),
                reason,
            },
            "operation dispatch-uncertain",
        )?;
        self.events
            .append_event(
                account_id,
                EVENT_TOPIC_OPERATION_DISPATCH_UNCERTAIN,
                None,
                Some(&message_id),
                payload,
            )
            .map_err(Into::into)
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
        FlushError::permanent(format!("invalid {:?} payload: {error}", operation.kind))
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
