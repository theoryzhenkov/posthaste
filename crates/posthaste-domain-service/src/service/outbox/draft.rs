//! Draft lifecycle: local-first save/discard as content ops whose visible
//! rows are DERIVED by replay from the op payload — a queued save is a visible
//! draft row immediately (no provider round trip, no sync lag), a queued
//! discard removes the op so the next replay forgets the row, and base stays
//! sync-owned. Same-key saves COALESCE (last-writer-wins per compose session);
//! flush-time stable-key → live-id resolution is the one registry seam. A
//! draft's identity NEVER rotates: one stable key from the first keystroke
//! through adoption, so a superseded/cancelled authoring path cannot strand a
//! row — the row is a pure function of the op, and no op means no row.

use super::classify::FlushError;
use crate::service::*;
use posthaste_domain_model::CommandAck;

impl MailService {
    /// Save a draft local-first: enqueue (or coalesce into) a draft
    /// create/update operation and re-derive its visible row from the overlay
    /// plane, then emit the projection echo — the draft appears in Drafts the
    /// moment this returns, materialized by replay from the op payload.
    ///
    /// `draft_key` is `None` for a brand-new draft (a stable local key is
    /// minted) or the draft's stable key for an edit. A save whose key already
    /// has a still-queued save REPLACES that op's payload in place (same op id
    /// — the create idempotency identity — and kind), so the outbox holds at
    /// most one queued save per compose session.
    ///
    /// @spec docs/L1-outbox#operation-model
    /// @spec docs/L1-outbox#temp-id-reconciliation
    pub async fn save_draft(
        &self,
        account_id: &AccountId,
        draft_key: Option<MessageId>,
        mut request: SendMessageRequest,
    ) -> Result<(Operation, Vec<DomainEvent>), ServiceError> {
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
        // The op carries the STABLE key as its entity id — the live id is
        // resolved at flush, immediately before the gateway call, so the push
        // always targets the freshest mapping the registry knows (a rotation
        // observed between enqueue and flush cannot stale it). Enqueue-time
        // resolution only picks the kind, and REGISTERS the key
        // (reserve-at-admission): an unknown key self-maps until the first
        // flush assigns a provider id.
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
        // Coalescing: replace the still-queued save's payload in place. The
        // guarded swap races the flusher's claim with exactly one winner — a
        // claimed (inflight) save is never rewritten mid-push; the loser falls
        // through and enqueues a fresh op.
        let mut coalesced: Option<Operation> = None;
        if let Some(queued) = self
            .outbox
            .list_pending_operations(account_id)?
            .into_iter()
            .rfind(|existing| {
                existing.entity.kind == OperationEntityKind::Draft
                    && existing.entity.id == key
                    && existing.kind.is_draft_save()
                    && existing.state == OperationState::Pending
            })
        {
            if self
                .outbox
                .replace_operation_payload(&queued.id, &payload)?
            {
                coalesced = self.outbox.get_operation(&queued.id)?;
            }
        }
        let operation = match coalesced {
            Some(operation) => operation,
            None => self.queue_operation(
                account_id,
                OperationEntity {
                    kind: OperationEntityKind::Draft,
                    id: key.clone(),
                },
                kind,
                payload,
                None,
                None,
            )?,
        };
        // Instant draft: re-derive the row from the queued save and echo the
        // projection from the effective read — the Drafts list row exists
        // before any provider call, materialized by replay from the op.
        let live_id = self.live_draft_id(account_id, &key)?;
        self.refresh_message_overlay(account_id, &live_id).await?;
        let mut events = Vec::new();
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
        Ok((operation, events))
    }

    /// Delete a draft local-first: enqueue a draft delete operation carrying
    /// the draft's STABLE key, re-derive its row (a tombstone hides it
    /// immediately), and emit the reconciling deletion echo. The live entity id
    /// is resolved at flush, immediately before the provider destroy, so a
    /// rotation observed between enqueue and flush retargets the destroy to the
    /// current live draft.
    ///
    /// The registry mapping is NOT forgotten here: identity survives until the
    /// destruction is confirmed — at this op's settlement or at sync-observed
    /// disappearance — so an in-flight op never references a forgotten mapping.
    ///
    /// `idempotent_redelivery` records whether a provider `notFound` at flush
    /// time is a benign already-gone (the send-consume settlement effect) or a
    /// genuine failure a user-initiated discard must surface. It is stamped
    /// onto the op so the gateway narrows its `notFound ⇒ Ok` mask to the
    /// idempotent case only.
    ///
    /// @spec docs/L1-outbox#operation-model
    pub async fn delete_draft(
        &self,
        account_id: &AccountId,
        draft_key: MessageId,
        idempotent_redelivery: bool,
    ) -> Result<(Operation, Vec<DomainEvent>), ServiceError> {
        let key = draft_key.to_string();
        // Reserve-at-admission, delete half: a key the registry does not know
        // yet (a headerless legacy/foreign draft addressed by its provider id)
        // self-maps here, so the flush-time resolve ALWAYS finds a mapping and a
        // typed miss there can only mean confirmed destruction — never "this key
        // was simply never registered".
        if self
            .draft_registry
            .resolve_draft_entity(account_id, &key)?
            .is_none()
        {
            self.draft_registry
                .set_draft_alias(account_id, &key, &key)?;
        }
        let live_id = self.live_draft_id(account_id, &key)?;
        // Effective membership BEFORE the fold: the deletion echo's mailbox
        // scope, and the "was anything visible?" gate for emitting it.
        let previous = self
            .message_detail_reader
            .get_message_summary(account_id, &live_id)?;
        let operation = self.queue_operation(
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Draft,
                id: key,
            },
            OperationKind::DraftDelete,
            serde_json::json!({ "idempotentRedelivery": idempotent_redelivery }),
            None,
            None,
        )?;
        // The tombstone fold: the draft leaves every effective view NOW; the
        // provider destroy follows through the outbox discipline.
        self.refresh_message_overlay(account_id, &live_id).await?;
        let mut events = Vec::new();
        if let Some(previous) = previous {
            events.push(self.events.append_event(
                account_id,
                EVENT_TOPIC_MESSAGE_UPDATED,
                previous.mailbox_ids.first(),
                Some(&live_id),
                serde_json::json!({ "messageId": live_id.as_str(), "deleted": true }),
            )?);
        }
        Ok((operation, events))
    }

    /// Discard a draft, user-initiated.
    ///
    /// Still-queued saves for the key are superseded (removed) — the discard
    /// wins the compose session. A draft that never reached the provider (no
    /// base row, no save ever in flight, never rotated) is discarded entirely
    /// locally: the save ops are gone, so the next replay forgets its derived
    /// row — no provider op at all, no unwind bookkeeping. Otherwise the
    /// provider destroy is enqueued non-idempotent so a `notFound` surfaces. A
    /// key that no longer names a live visible row is a surfaced `NotFound`,
    /// not a silent success. Base is untouched either way.
    pub async fn discard_draft(
        &self,
        account_id: &AccountId,
        draft_key: MessageId,
    ) -> Result<CommandAck, ServiceError> {
        let key = draft_key.to_string();
        let resolved = self.draft_registry.resolve_draft_entity(account_id, &key)?;
        let live = resolved.clone().unwrap_or_else(|| key.clone());
        let live_id = MessageId::from(live.as_str());
        // A discard of a draft with no live visible row must surface — the
        // client reverts the optimistic fold and shows the error. The effective
        // read covers both a synced draft and a queued-only one.
        let summary = self
            .message_detail_reader
            .get_message_summary(account_id, &live_id)?;
        if summary.is_none() {
            return Err(ServiceError::from(StoreError::NotFound(format!(
                "draft:{}",
                live_id.as_str()
            ))));
        }
        // Supersede queued saves: each removal is the same guarded
        // cancel-vs-flush statement the user-cancel path uses, so a save the
        // flusher claimed concurrently survives (and marks the provider as
        // possibly holding the draft).
        let mut save_may_have_reached_provider = false;
        for existing in self.outbox.list_pending_operations(account_id)? {
            if existing.entity.kind != OperationEntityKind::Draft
                || existing.entity.id != key
                || !existing.kind.is_draft_save()
            {
                continue;
            }
            match existing.state {
                OperationState::Pending | OperationState::Failed => {
                    if !self.outbox.remove_operation_unless_inflight(&existing.id)? {
                        save_may_have_reached_provider = true;
                    }
                }
                _ => save_may_have_reached_provider = true,
            }
        }
        let base_has_row = {
            let overlay = self.overlay.clone();
            let owned_account = account_id.clone();
            let owned_message = live_id.clone();
            offload(move || overlay.read_base_message_record(&owned_account, &owned_message))
                .await?
                .is_some()
        };
        let rotated = resolved.as_deref().is_some_and(|entity| entity != key);
        if base_has_row || save_may_have_reached_provider || rotated {
            // The provider (or an in-flight save) may hold the draft: queue
            // the non-idempotent destroy; the tombstone fold hides the row
            // immediately.
            let (_operation, events) = self.delete_draft(account_id, draft_key, false).await?;
            return Ok(CommandAck { events });
        }
        // Never reached the provider: a purely local discard. The queued saves
        // are already removed above, so the draft's derived row is gone once
        // replay runs — no op, no row. Forget the reserved mapping, re-derive
        // (removing the now-ownerless entry), then echo the deletion.
        self.draft_registry.remove_draft_alias(account_id, &key)?;
        self.refresh_message_overlay(account_id, &live_id).await?;
        let event = self.events.append_event(
            account_id,
            EVENT_TOPIC_MESSAGE_UPDATED,
            summary
                .as_ref()
                .and_then(|summary| summary.mailbox_ids.first()),
            Some(&live_id),
            serde_json::json!({ "messageId": live_id.as_str(), "deleted": true }),
        )?;
        Ok(CommandAck {
            events: vec![event],
        })
    }

    /// The live id a stable draft key currently maps to — the id its visible
    /// row is keyed by (the key itself until a flush assigns a provider id).
    fn live_draft_id(
        &self,
        account_id: &AccountId,
        draft_key: &str,
    ) -> Result<MessageId, ServiceError> {
        Ok(MessageId::from(
            self.draft_registry
                .resolve_draft_entity(account_id, draft_key)?
                .unwrap_or_else(|| draft_key.to_string())
                .as_str(),
        ))
    }

    /// Whether `draft_key` names a message already in the projection — i.e. a
    /// draft being resumed by its provider id rather than a freshly minted
    /// local key. Used to edit such a draft in place instead of duplicating it.
    /// Mailbox membership is the light existence proxy: a draft always sits in
    /// the Drafts mailbox.
    pub(super) fn draft_message_exists(
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

    /// The eager ensure-draft for HELD sends: during the hold the message is a
    /// REAL provider draft, visible and editable on every device. Runs each
    /// flush pass, before the readiness-gated drain; the registry is the step
    /// ledger — done = the compose key maps to a provider id (a crash retry
    /// re-creates under the same deterministic create-id, so the server dedups).
    /// A queued save op on the same key IS the ensure step (it flushes eagerly
    /// on its own), and a failure here only warns — cross-device visibility
    /// degrades, the submit step is untouched.
    ///
    /// The visible row is DERIVED: this step provider-creates the draft and
    /// repoints the registry key → provider id, and `refresh_message_overlay`
    /// then re-materializes the held send's draft-form row at the new id from
    /// the send op's own payload (the fold follows the rotation via the
    /// registry). No row is written directly here.
    pub(super) async fn ensure_drafts_for_held_sends(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
        wall_now: &str,
        mono_now: i64,
        events: &mut Vec<DomainEvent>,
    ) -> Result<(), ServiceError> {
        let pending = self.outbox.list_pending_operations(account_id)?;
        for operation in &pending {
            if operation.kind != OperationKind::Send || operation.state != OperationState::Pending {
                continue;
            }
            let held = operation
                .send_at
                .as_deref()
                .is_some_and(|send_at| send_at > wall_now)
                || operation
                    .hold_until_mono
                    .is_some_and(|hold| hold > mono_now);
            if !held {
                continue;
            }
            let Ok(posthaste_domain_model::MailIntent::Send(request)) = operation.intent() else {
                continue;
            };
            let Some(key) = request
                .draft_id
                .as_deref()
                .map(str::trim)
                .filter(|key| !key.is_empty())
            else {
                continue;
            };
            match self
                .draft_registry
                .resolve_draft_entity(account_id, key)?
                .as_deref()
            {
                // Confirmed destroyed since admission: nothing to ensure.
                None => continue,
                // Rotated to a provider id: the step is complete.
                Some(live) if live != key => continue,
                _ => {}
            }
            let save_queued = pending.iter().any(|other| {
                other.entity.kind == OperationEntityKind::Draft
                    && other.entity.id == key
                    && other.kind.is_draft_save()
                    && matches!(
                        other.state,
                        OperationState::Pending | OperationState::Inflight
                    )
            });
            if save_queued {
                continue;
            }
            match gateway
                .save_draft(account_id, &request, None, false, operation.id.as_str())
                .await
            {
                Ok(new_id) => {
                    // The rotation write IS the durable step-complete marker,
                    // and the identity bridge: the held send's draft-form row
                    // now derives at the provider id via the fold. Re-derive
                    // both ends of the rotation and echo the row's move.
                    let rotated = new_id.as_str() != key;
                    self.draft_registry
                        .set_draft_alias(account_id, key, new_id.as_str())?;
                    if rotated {
                        let diff = self
                            .refresh_message_overlay(account_id, &MessageId::from(key))
                            .await?;
                        if diff.effectively_retired() {
                            events.push(self.events.append_event(
                                account_id,
                                EVENT_TOPIC_MESSAGE_UPDATED,
                                None,
                                Some(&MessageId::from(key)),
                                serde_json::json!({ "messageId": key, "deleted": true }),
                            )?);
                        }
                    }
                    let live_id = MessageId::from(new_id.as_str());
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
                Err(error) => {
                    ph_warn!(
                        events::OUTBOX_HELD_SEND_ENSURE_FAILED,
                        account_id = %account_id,
                        operation_id = %operation.id,
                        error = %error,
                        "held send's eager ensure-draft failed; cross-device \
                         visibility degraded until the next flush window"
                    );
                }
            }
        }
        Ok(())
    }

    /// Resolve a draft op's stable key to the live entity id at flush time —
    /// immediately before the gateway call — so the push targets the freshest
    /// mapping the registry knows. This closes the in-flight-op vs sync race: a
    /// sync chunk that repointed the registry (a rotation observed from another
    /// device) between enqueue and flush is reflected in the target.
    ///
    /// `None` is the TYPED miss: the registry forgets only on CONFIRMED
    /// destruction, so a miss means the draft is gone — the caller decides what
    /// its op means then (a save re-creates; a discard settles as already-done).
    pub(super) fn resolve_draft_flush_target(
        &self,
        account_id: &AccountId,
        draft_key: &str,
    ) -> Result<Option<String>, FlushError> {
        self.draft_registry
            .resolve_draft_entity(account_id, draft_key)
            .map_err(|error| {
                FlushError::permanent(format!("draft identity resolution failed: {error}"))
            })
    }

    /// Post-settlement overlay maintenance for a CONTENT op (a draft save or a
    /// send): the op does not leave the log — it rests settled (`applied`),
    /// still folded — so its visible row is re-derived, not written. A JMAP
    /// draft update rotates the provider id (create-new + destroy-old), so the
    /// derived row moves: at the OLD live id the fold no longer lands (the row
    /// retires — prune echo), and at the NEW id the still-settled op folds (the
    /// row materializes there — projection echo). A send does not rotate; its
    /// single Sent row re-derives at the same id. The row leaves the log only
    /// by causal truncation, once its provider copy is confirmed into base.
    ///
    /// A settled send also CONSUMES its originating draft: the gateway destroyed
    /// the provider copy inside the send's own execution, so the registry
    /// mapping is forgotten here (confirmed destruction — a later save/redeliver
    /// resolves nothing, never a double-destroy) and the draft's live row is
    /// tombstoned/dropped with a deletion echo.
    pub(super) async fn settle_content_op_overlay(
        &self,
        account_id: &AccountId,
        operation: &Operation,
        old_live: &str,
        new_live: &str,
        events: &mut Vec<DomainEvent>,
    ) -> Result<(), ServiceError> {
        // A settled send: retire the consumed draft (confirmed destroyed) —
        // forget the registry mapping BEFORE re-deriving, so the send op's fold
        // no longer re-tombstones the resolved live id, and hide/drop that live
        // row until sync prunes the provider copy from base.
        if operation.kind == OperationKind::Send {
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
                    self.draft_registry.remove_draft_alias(account_id, key)?;
                    let base_has_row = {
                        let overlay = self.overlay.clone();
                        let owned_account = account_id.clone();
                        let owned_message = live_id.clone();
                        offload(move || {
                            overlay.read_base_message_record(&owned_account, &owned_message)
                        })
                        .await?
                        .is_some()
                    };
                    {
                        let overlay = self.overlay.clone();
                        let owned_account = account_id.clone();
                        let owned_message = live_id.clone();
                        offload(move || {
                            if base_has_row {
                                overlay.tombstone_overlay_message(&owned_account, &owned_message)
                            } else {
                                overlay.remove_overlay_message(&owned_account, &owned_message)
                            }
                        })
                        .await?;
                    }
                    events.push(self.events.append_event(
                        account_id,
                        EVENT_TOPIC_MESSAGE_UPDATED,
                        None,
                        Some(&live_id),
                        serde_json::json!({ "messageId": live_id.as_str(), "deleted": true }),
                    )?);
                }
            }
        }
        // The old live id, if the provider rotated it away: the fold now lands
        // at the new id, so re-derive the old one to retire its stale row.
        if old_live != new_live {
            let old_id = MessageId::from(old_live);
            let diff = self.refresh_message_overlay(account_id, &old_id).await?;
            if diff.effectively_retired() {
                events.push(self.events.append_event(
                    account_id,
                    EVENT_TOPIC_MESSAGE_UPDATED,
                    None,
                    Some(&old_id),
                    serde_json::json!({ "messageId": old_live, "deleted": true }),
                )?);
            }
        }
        // Re-derive every row the settled op still folds (the new draft id, or
        // the send's Sent row + consumed-draft tombstone) and project the
        // survivors so the client swaps rows instantly instead of waiting for
        // the next sync.
        for row_id in self.op_touched_row_ids(account_id, operation)? {
            self.refresh_message_overlay(account_id, &row_id).await?;
            if let Some(summary) = self
                .message_detail_reader
                .get_message_summary(account_id, &row_id)?
            {
                let scope = summary.mailbox_ids.first().cloned();
                events.push(self.events.append_event(
                    account_id,
                    EVENT_TOPIC_MESSAGE_UPDATED,
                    scope.as_ref(),
                    Some(&row_id),
                    serde_json::json!({
                        "messageId": row_id.as_str(),
                        "changes": { "mailboxes": true },
                        "projection": &summary,
                    }),
                )?);
            }
        }
        Ok(())
    }
}
