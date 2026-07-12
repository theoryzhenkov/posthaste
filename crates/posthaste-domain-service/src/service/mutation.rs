use posthaste_domain_model::{
    AccountId, AddToMailboxCommand, CommandAck, DomainEvent, MailboxId, MessageChangeAssertion,
    MessageId, MessageRecord, Operation, OperationEntity, OperationEntityKind, OperationKind,
    OperationState, RemoveFromMailboxCommand, ReplaceMailboxesCommand, SendMessageRequest,
    ServiceError, SetKeywordsCommand, StoreError, ThreadId, EVENT_TOPIC_MESSAGE_UPDATED,
};
use serde_json::json;

use super::message_queries::{intent_fold_effect, project_record, FoldEffect};
use super::{encode_payload, offload, MailService};

/// How a refresh treats an overlay entry whose ops have ALL settled.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayRetire {
    /// Base was just made authoritative for this message (a provider readback
    /// or a rejection was written): remove the entry unconditionally.
    Immediate,
    /// Base may not have absorbed the settled effect yet (no-readback
    /// settlement, e.g. IMAP; or a periodic sweep): remove the entry only once
    /// base COVERS its fold (tombstone: only once the base row is gone).
    /// Retire-on-confirmation — prevents the settle→next-sync revert flicker.
    ConfirmAgainstBase,
}

/// The role a visible row plays for an entity-intent op in the fold (NS2
/// Slice 4): draft ops address their key's live row; a send addresses TWO
/// rows — its own provisional Sent row and the consumed draft's live row.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EntityFoldRole {
    /// The row a draft save/discard's stable key currently maps to.
    DraftKey,
    /// The send op's own entity id: the provisional Sent row.
    SendRow,
    /// The live row of the draft a send consumes.
    SendConsumedDraft,
}

/// Synthesize the provisional Sent overlay row a due/dispatched send folds to
/// (NS2 Slice 4, D172): visible in Sent from dispatch, adopted (retired) once
/// sync lands the provider copy — matched by the transport-shared
/// `Message-ID` prefix ([`posthaste_domain_model::send_identity_prefix`]).
/// The domain half of the stamped id is a best-effort guess from the sender
/// (adoption ignores it).
pub(crate) fn synthesize_sent_record(
    request: &SendMessageRequest,
    operation: &Operation,
    sent_mailbox: Option<&MailboxId>,
    row_id: &MessageId,
) -> MessageRecord {
    const PREVIEW_CHARS: usize = 180;
    let preview: String = request.body.trim().chars().take(PREVIEW_CHARS).collect();
    let token = posthaste_domain_model::send_identity_token(operation.id.as_str());
    let domain = request
        .from
        .as_ref()
        .and_then(|recipient| recipient.email.rsplit_once('@'))
        .map(|(_, domain)| domain)
        .filter(|domain| !domain.is_empty())
        .unwrap_or("posthaste.local");
    MessageRecord {
        id: row_id.clone(),
        source_thread_id: ThreadId::from(row_id.as_str()),
        remote_blob_id: None,
        subject: {
            let subject = request.subject.trim();
            (!subject.is_empty()).then(|| subject.to_string())
        },
        from_name: request
            .from
            .as_ref()
            .and_then(|recipient| recipient.name.clone()),
        from_email: request
            .from
            .as_ref()
            .map(|recipient| recipient.email.clone()),
        to: request.to.clone(),
        preview: (!preview.is_empty()).then_some(preview),
        received_at: operation.updated_at.clone(),
        has_attachment: !request.attachments.is_empty(),
        size: request.body.len() as i64,
        mailbox_ids: sent_mailbox
            .map(|mailbox_id| vec![mailbox_id.clone()])
            .unwrap_or_default(),
        // Own sent mail is read (IMAP/JMAP convention).
        keywords: vec!["$seen".to_string()],
        body_html: None,
        body_text: None,
        raw_mime: None,
        rfc_message_id: Some(format!("{token}@{domain}")),
        in_reply_to: request.in_reply_to.clone(),
        references: request
            .references
            .as_deref()
            .map(|references| references.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default(),
        draft_id: None,
        list_unsubscribe: None,
    }
}

/// Synthesize the overlay row a queued draft save folds to (NS2 Slice 3): the
/// draft is VISIBLE the moment its op is queued — no provider round trip, no
/// sync lag. Body fields stay `None` (the overlay never carries bodies); the
/// queued request itself is the content authority until the save settles
/// (`get_draft_content` reads it back from the op payload).
pub(crate) fn synthesize_draft_record(
    prior: Option<MessageRecord>,
    request: &SendMessageRequest,
    operation: &Operation,
    drafts_mailbox: Option<&MailboxId>,
    live_id: &MessageId,
    stable_key: &str,
) -> MessageRecord {
    const PREVIEW_CHARS: usize = 180;
    let preview: String = request.body.trim().chars().take(PREVIEW_CHARS).collect();
    // The Drafts mailbox by role; a prior row's membership as fallback (e.g. a
    // pre-discovery save) so the row does not vanish from every list.
    let mailbox_ids = drafts_mailbox
        .map(|mailbox_id| vec![mailbox_id.clone()])
        .or_else(|| prior.as_ref().map(|record| record.mailbox_ids.clone()))
        .unwrap_or_default();
    MessageRecord {
        id: live_id.clone(),
        source_thread_id: prior
            .as_ref()
            .map(|record| record.source_thread_id.clone())
            .unwrap_or_else(|| ThreadId::from(live_id.as_str())),
        remote_blob_id: None,
        subject: {
            let subject = request.subject.trim();
            (!subject.is_empty()).then(|| subject.to_string())
        },
        from_name: request
            .from
            .as_ref()
            .and_then(|recipient| recipient.name.clone()),
        from_email: request
            .from
            .as_ref()
            .map(|recipient| recipient.email.clone()),
        to: request.to.clone(),
        preview: (!preview.is_empty()).then_some(preview),
        // Each coalesced save bumps the op's `updated_at`, so the draft sorts
        // like real autosave (most recently edited first).
        received_at: operation.updated_at.clone(),
        has_attachment: !request.attachments.is_empty(),
        size: request.body.len() as i64,
        mailbox_ids,
        // `$seen` matches the gateways' save semantics (an own draft is never
        // "new mail"); the retire compare treats it as soft (IMAP appends
        // `\Draft` only).
        keywords: vec!["$draft".to_string(), "$seen".to_string()],
        body_html: None,
        body_text: None,
        raw_mime: None,
        rfc_message_id: None,
        in_reply_to: request.in_reply_to.clone(),
        references: request
            .references
            .as_deref()
            .map(|references| references.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default(),
        draft_id: Some(stable_key.to_string()),
        list_unsubscribe: None,
    }
}

impl MailService {
    /// Re-derive one visible row's OVERLAY entry from base + its unsettled
    /// intents (NS1 D167, extended to the draft plane in NS2 Slice 3): the
    /// single maintenance function for the optimistic plane, called at every
    /// lifecycle moment that can change the fold's inputs — mutation queue,
    /// draft save/discard queue, op settlement, and the post-sync sweep.
    ///
    /// `message_id` is the row's LIVE id: for message assertions the op's
    /// entity id; for draft intents the registry-resolved live id their stable
    /// key currently maps to (draft ops carry the KEY, the overlay is keyed by
    /// the visible row).
    ///
    /// No unsettled ops → the entry is removed (base shows through), subject
    /// to `retire`. Folded-to-removed → tombstone. A discard folding over no
    /// base row removes the entry (nothing to hide). No base row otherwise →
    /// any existing entry stays as the last-known fold (e.g. a pending flag
    /// racing a remote delete) until its op settles and this runs again.
    pub(crate) async fn refresh_message_overlay(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        retire: OverlayRetire,
    ) -> Result<(), ServiceError> {
        let overlay = self.overlay.clone();
        let unsettled = {
            let outbox = self.outbox.clone();
            let account_id = account_id.clone();
            offload(move || outbox.list_unsettled_operations(&account_id)).await?
        };
        let mut message_ops: Vec<Operation> = Vec::new();
        // Entity-intent ops relevant to this row, in insertion order, tagged
        // with the ROLE this row plays for them (NS2 Slice 4: a send is
        // multi-row — its own provisional Sent row AND the consumed draft's
        // live row).
        let mut entity_ops: Vec<(Operation, EntityFoldRole)> = Vec::new();
        for op in unsettled {
            if !matches!(
                op.state,
                OperationState::Pending | OperationState::Inflight | OperationState::Applied
            ) {
                continue;
            }
            match op.entity.kind {
                OperationEntityKind::Message if op.kind == OperationKind::Send => {
                    if op.entity.id == message_id.as_str() {
                        entity_ops.push((op, EntityFoldRole::SendRow));
                    } else if let Ok(Some(FoldEffect::SendEffects {
                        consumes_draft_key: Some(key),
                        ..
                    })) = intent_fold_effect(&op)
                    {
                        let consumed_live = self
                            .draft_registry
                            .resolve_draft_entity(account_id, &key)?
                            .unwrap_or(key);
                        if consumed_live == message_id.as_str() {
                            entity_ops.push((op, EntityFoldRole::SendConsumedDraft));
                        }
                    }
                }
                OperationEntityKind::Message => {
                    if op.kind.is_state_assertion() && op.entity.id == message_id.as_str() {
                        message_ops.push(op);
                    }
                }
                OperationEntityKind::Draft => {
                    let live = self
                        .draft_registry
                        .resolve_draft_entity(account_id, &op.entity.id)?
                        .unwrap_or_else(|| op.entity.id.clone());
                    if live == message_id.as_str() {
                        entity_ops.push((op, EntityFoldRole::DraftKey));
                    }
                }
            }
        }
        if message_ops.is_empty() && entity_ops.is_empty() {
            if retire == OverlayRetire::ConfirmAgainstBase {
                let confirmed = {
                    let overlay = overlay.clone();
                    let account_id = account_id.clone();
                    let message_id = message_id.clone();
                    offload(move || {
                        let Some(entry) = overlay.read_overlay_message(&account_id, &message_id)?
                        else {
                            return Ok::<bool, StoreError>(true); // nothing to retire
                        };
                        let base = overlay.read_base_message_record(&account_id, &message_id)?;
                        Ok(match (entry, base) {
                            // Tombstone: confirmed once the base row is gone.
                            (None, base) => base.is_none(),
                            // Folded row: confirmed once base carries the same
                            // keyword + mailbox sets. A draft row compares
                            // keywords modulo `$seen`: IMAP appends `\Draft`
                            // only, so the synthesized `$seen` would otherwise
                            // never be covered and the entry would linger.
                            (Some(mut folded), Some(mut base)) => {
                                folded.keywords.sort();
                                base.keywords.sort();
                                folded.mailbox_ids.sort();
                                base.mailbox_ids.sort();
                                let keywords_covered = if folded.draft_id.is_some() {
                                    let soft = |keywords: &[String]| {
                                        keywords
                                            .iter()
                                            .filter(|keyword| keyword.as_str() != "$seen")
                                            .cloned()
                                            .collect::<Vec<_>>()
                                    };
                                    soft(&folded.keywords) == soft(&base.keywords)
                                } else {
                                    folded.keywords == base.keywords
                                };
                                keywords_covered && folded.mailbox_ids == base.mailbox_ids
                            }
                            // Folded row but no base row: not yet confirmed.
                            (Some(_), None) => false,
                        })
                    })
                    .await?
                };
                if !confirmed {
                    return Ok(());
                }
            }
            let account_id = account_id.clone();
            let message_id = message_id.clone();
            offload(move || overlay.remove_overlay_message(&account_id, &message_id)).await?;
            return Ok(());
        }
        let base = {
            let overlay = overlay.clone();
            let account_id = account_id.clone();
            let message_id = message_id.clone();
            offload(move || overlay.read_base_message_record(&account_id, &message_id)).await?
        };
        // Entity-plane fold first (each effect is total — D172), in insertion
        // order: the last queued save wins the row's content; a discard (or a
        // due send's consume) folds the row away; a due send upserts its
        // provisional Sent row. Message assertions then replay on top of
        // whatever row survives. Send phase (D172, phase-aware): a HELD send
        // folds NOTHING — the draft stays visible and cancelable; the flip to
        // tombstone+sent needs no new trigger (coming due re-derives via the
        // flush/settle refreshes).
        let needs_clocks = entity_ops
            .iter()
            .any(|(op, _)| op.kind == OperationKind::Send);
        let (wall_now, mono_now) = if needs_clocks {
            let wall = super::outbox::schedule::wall_now_rfc3339().map_err(|error| {
                ServiceError::from(posthaste_domain_model::GatewayError::Rejected(error))
            })?;
            (wall, super::outbox::schedule::monotonic_now_secs())
        } else {
            (String::new(), 0)
        };
        let send_is_held = |op: &Operation| {
            op.state == OperationState::Pending
                && (op
                    .send_at
                    .as_deref()
                    .is_some_and(|send_at| send_at > wall_now.as_str())
                    || op.hold_until_mono.is_some_and(|hold| hold > mono_now))
        };
        let mut state = base.clone();
        let mut discarded_by_draft_op = false;
        let mut drafts_mailbox: Option<Option<MailboxId>> = None;
        let mut sent_mailbox: Option<Option<MailboxId>> = None;
        for (op, role) in &entity_ops {
            match (intent_fold_effect(op)?, role) {
                (Some(FoldEffect::UpsertDraft(request)), EntityFoldRole::DraftKey) => {
                    let drafts_mailbox = match &drafts_mailbox {
                        Some(resolved) => resolved.clone(),
                        None => drafts_mailbox
                            .insert(self.drafts_mailbox_id(account_id)?)
                            .clone(),
                    };
                    state = Some(synthesize_draft_record(
                        state.take(),
                        &request,
                        op,
                        drafts_mailbox.as_ref(),
                        message_id,
                        &op.entity.id,
                    ));
                    discarded_by_draft_op = false;
                }
                (Some(FoldEffect::TombstoneDraft), EntityFoldRole::DraftKey) => {
                    state = None;
                    discarded_by_draft_op = true;
                }
                (Some(FoldEffect::SendEffects { request, .. }), EntityFoldRole::SendRow) => {
                    if !send_is_held(op) {
                        let sent_mailbox = match &sent_mailbox {
                            Some(resolved) => resolved.clone(),
                            None => sent_mailbox
                                .insert(self.mailbox_id_by_role(account_id, "sent")?)
                                .clone(),
                        };
                        state = Some(synthesize_sent_record(
                            &request,
                            op,
                            sent_mailbox.as_ref(),
                            message_id,
                        ));
                        discarded_by_draft_op = false;
                    }
                }
                (
                    Some(FoldEffect::SendEffects {
                        request,
                        consumes_draft_key,
                    }),
                    EntityFoldRole::SendConsumedDraft,
                ) => {
                    if send_is_held(op) {
                        // D172 as ratified: a HELD send folds a DRAFT-FORM
                        // row — the hold's content is visible and cancelable
                        // even with no client-side save (the eager ensure
                        // step mirrors it provider-side, D173).
                        let drafts_mailbox = match &drafts_mailbox {
                            Some(resolved) => resolved.clone(),
                            None => drafts_mailbox
                                .insert(self.drafts_mailbox_id(account_id)?)
                                .clone(),
                        };
                        let key = consumes_draft_key.unwrap_or_default();
                        state = Some(synthesize_draft_record(
                            state.take(),
                            &request,
                            op,
                            drafts_mailbox.as_ref(),
                            message_id,
                            &key,
                        ));
                        discarded_by_draft_op = false;
                    } else {
                        // The due send consumes its draft: the row leaves
                        // Drafts optimistically with the dispatch.
                        state = None;
                        discarded_by_draft_op = true;
                    }
                }
                _ => {}
            }
        }
        let base_exists = base.is_some();
        let account_id = account_id.clone();
        let message_id = message_id.clone();
        match state {
            Some(record) => match project_record(record, &message_ops)? {
                Some(folded) => {
                    offload(move || overlay.upsert_overlay_message(&account_id, &folded)).await?;
                }
                None => {
                    offload(move || overlay.tombstone_overlay_message(&account_id, &message_id))
                        .await?;
                }
            },
            None if discarded_by_draft_op => {
                if base_exists {
                    // Hide the base row until the provider destroy is
                    // confirmed into base by sync.
                    offload(move || overlay.tombstone_overlay_message(&account_id, &message_id))
                        .await?;
                } else {
                    // Discard of a never-synced draft: nothing to hide.
                    offload(move || overlay.remove_overlay_message(&account_id, &message_id))
                        .await?;
                }
            }
            None => {
                if message_ops
                    .iter()
                    .any(|op| op.kind == OperationKind::Destroy)
                {
                    offload(move || overlay.tombstone_overlay_message(&account_id, &message_id))
                        .await?;
                }
            }
        }
        Ok(())
    }

    /// The account's mailbox id for a role, when discovered.
    pub(crate) fn mailbox_id_by_role(
        &self,
        account_id: &AccountId,
        role: &str,
    ) -> Result<Option<MailboxId>, ServiceError> {
        Ok(self
            .mailbox_reader
            .list_mailboxes(account_id)?
            .into_iter()
            .find(|mailbox| mailbox.role.as_deref() == Some(role))
            .map(|mailbox| mailbox.id))
    }

    /// The account's Drafts mailbox id by role, when discovered.
    pub(crate) fn drafts_mailbox_id(
        &self,
        account_id: &AccountId,
    ) -> Result<Option<MailboxId>, ServiceError> {
        self.mailbox_id_by_role(account_id, "drafts")
    }
    fn queue_message_operation(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        kind: OperationKind,
        payload: serde_json::Value,
    ) -> Result<Operation, ServiceError> {
        self.queue_operation(
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Message,
                id: message_id.to_string(),
            },
            kind,
            payload,
            None,
            None,
        )
    }

    fn remove_operation_after_local_failure(
        &self,
        operation: &posthaste_domain_model::Operation,
        error: ServiceError,
    ) -> ServiceError {
        let _ = self.outbox.remove_operation(&operation.id);
        error
    }

    async fn queue_then_emit_message_operation(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        kind: OperationKind,
        payload: serde_json::Value,
    ) -> Result<CommandAck, ServiceError> {
        // NS1 cutover: the op is queued, the OVERLAY plane is refreshed (the
        // fold's output lands in message_overlay — base is untouched; sync is
        // its only writer), and the enriched `message.updated` echo is built
        // from the EFFECTIVE read — the same folded derivation every SQL read
        // serves, so the echo, the lists, and the counts cannot disagree. The
        // event shape matches the old write-through's echo (projection = the
        // body-free `MessageSummary`), so the client entity store ingests it
        // identically → the mail-list row moves on the echo (sub-second).
        //
        // Mailbox COUNTS ride no event (RFC-L2-count-unification): a client
        // reacts to the echo by invalidating its mailbox-count query and
        // re-reading `list_mailboxes`, which derives counts live over the same
        // effective plane this fold just changed.
        //
        // On a local write failure, retract the op so the outbox and overlay
        // do not diverge.
        let operation = self.queue_message_operation(account_id, message_id, kind, payload)?;
        let events = match self
            .apply_assertion_to_overlay(account_id, message_id, &operation)
            .await
        {
            Ok(events) => events,
            Err(error) => {
                return Err(self.remove_operation_after_local_failure(&operation, error));
            }
        };
        Ok(CommandAck { events })
    }

    /// Fold the just-queued assertion into the overlay plane and emit the
    /// enriched echo event from the effective read (NS1 — replaces the S2
    /// canonical write-through: base is no longer touched by mutations).
    async fn apply_assertion_to_overlay(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        operation: &Operation,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        if !operation.kind.is_state_assertion() {
            return Ok(Vec::new());
        }
        // Effective membership BEFORE the fold: the destroy event's mailbox
        // scope and the replace event's `arrived` diff (parity with the old
        // path's canonical-before-write read).
        let previous = self
            .message_detail_reader
            .get_message_summary(account_id, message_id)?;
        if operation.kind == OperationKind::Destroy && previous.is_none() {
            return Err(ServiceError::from(StoreError::NotFound(format!(
                "message:{}",
                message_id.as_str()
            ))));
        }

        // Ops are non-empty here (one was just queued), so no retire decision
        // arises.
        self.refresh_message_overlay(account_id, message_id, OverlayRetire::Immediate)
            .await?;

        let (payload, scope_mailbox) = match operation.kind {
            OperationKind::SetKeywords => {
                let summary = self
                    .message_detail_reader
                    .get_message_summary(account_id, message_id)?
                    .ok_or_else(|| {
                        StoreError::NotFound(format!("message:{}", message_id.as_str()))
                    })?;
                let scope = summary.mailbox_ids.first().cloned();
                let assertion = MessageChangeAssertion::after(summary.clone());
                (
                    json!({
                        "messageId": message_id.as_str(),
                        "changes": { "keywords": true },
                        "keywords": summary.keywords,
                        "assertion": assertion,
                        "projection": &summary,
                    }),
                    scope,
                )
            }
            OperationKind::ReplaceMailboxes => {
                let posthaste_domain_model::MailIntent::ReplaceMailboxes(command) =
                    operation.intent().map_err(|error| {
                        ServiceError::from(posthaste_domain_model::GatewayError::Internal(error))
                    })?
                else {
                    unreachable!("guarded by operation.kind above");
                };
                // Parity with the write-through's store invariant: a mailbox
                // replace clears any snooze row (message.snooze re-inserts
                // after its own move). Local-plane write, not provider truth.
                self.snooze_reader.delete_snooze(account_id, message_id)?;
                let summary = self
                    .message_detail_reader
                    .get_message_summary(account_id, message_id)?
                    .ok_or_else(|| {
                        StoreError::NotFound(format!("message:{}", message_id.as_str()))
                    })?;
                let previous_set: std::collections::BTreeSet<&MailboxId> = previous
                    .as_ref()
                    .map(|summary| summary.mailbox_ids.iter().collect())
                    .unwrap_or_default();
                let arrived_mailbox_ids: Vec<&str> = command
                    .mailbox_ids
                    .iter()
                    .filter(|id| !previous_set.contains(id))
                    .map(MailboxId::as_str)
                    .collect();
                (
                    json!({
                        "messageId": message_id.as_str(),
                        "changes": {
                            "mailboxes": true,
                            "arrived": !arrived_mailbox_ids.is_empty(),
                        },
                        "mailboxIds": command.mailbox_ids.iter().map(MailboxId::as_str).collect::<Vec<_>>(),
                        "arrivedMailboxIds": arrived_mailbox_ids,
                        "projection": &summary,
                    }),
                    command.mailbox_ids.first().cloned(),
                )
            }
            OperationKind::Destroy => (
                json!({ "messageId": message_id.as_str(), "deleted": true }),
                previous
                    .as_ref()
                    .and_then(|summary| summary.mailbox_ids.first().cloned()),
            ),
            _ => unreachable!("guarded by is_state_assertion above"),
        };

        let event = self.events.append_event(
            account_id,
            EVENT_TOPIC_MESSAGE_UPDATED,
            scope_mailbox.as_ref(),
            Some(message_id),
            payload,
        )?;
        Ok(vec![event])
    }

    /// Add/remove JMAP keywords on a message, local-first.
    ///
    /// Enqueues a state assertion and reflects it through the read-time overlay;
    /// the authoritative projection remains sync-owned.
    ///
    /// @spec docs/L1-api#message-commands
    /// @spec docs/L1-outbox#operation-model
    pub async fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &SetKeywordsCommand,
    ) -> Result<CommandAck, ServiceError> {
        let payload = encode_payload(command, "keyword command")?;
        self.queue_then_emit_message_operation(
            account_id,
            message_id,
            OperationKind::SetKeywords,
            payload,
        )
        .await
    }

    /// Snooze scheduler: return every due snoozed message to the Inbox.
    /// Each move is the same `replace_mailboxes` path the client uses, so the
    /// provider move is enqueued (flushed on the next sync) + the snooze row
    /// clears immediately (no re-query next tick). Server-owned → cross-device
    /// coherent; not user-initiated → no undo step. Returns the count of
    /// messages returned.
    ///
    /// @spec docs/eph/DESIGN-L2-snooze
    pub async fn auto_return_snoozed_messages(
        &self,
        account_id: &AccountId,
        now: i64,
    ) -> Result<usize, ServiceError> {
        let due = self.snooze_reader.list_due_snoozes(account_id, now)?;
        if due.is_empty() {
            return Ok(0);
        }
        let inbox_id = self
            .mailbox_reader
            .list_mailboxes(account_id)?
            .into_iter()
            .find(|mailbox| mailbox.role.as_deref() == Some("inbox"))
            .map(|mailbox| mailbox.id);
        let Some(inbox_id) = inbox_id else {
            return Ok(0);
        };
        let mut returned = 0;
        for (message_id, _until) in due {
            if self
                .replace_mailboxes(
                    account_id,
                    &message_id,
                    &ReplaceMailboxesCommand {
                        mailbox_ids: vec![inbox_id.clone()],
                    },
                )
                .await
                .is_ok()
            {
                returned += 1;
            }
        }
        Ok(returned)
    }

    /// Atomically replace all mailbox memberships for a message, local-first.
    ///
    /// @spec docs/L1-api#message-commands
    /// @spec docs/L1-outbox#operation-model
    pub async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &ReplaceMailboxesCommand,
    ) -> Result<CommandAck, ServiceError> {
        let payload = encode_payload(command, "mailbox command")?;
        self.queue_then_emit_message_operation(
            account_id,
            message_id,
            OperationKind::ReplaceMailboxes,
            payload,
        )
        .await
    }

    /// Add a message to a mailbox (idempotent: no-op if already present).
    ///
    /// @spec docs/L1-api#message-commands
    pub async fn add_to_mailbox(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &AddToMailboxCommand,
    ) -> Result<CommandAck, ServiceError> {
        let mut mailbox_ids = self.list_message_mailboxes_with_overlay(account_id, message_id)?;
        if !mailbox_ids.contains(&command.mailbox_id) {
            mailbox_ids.push(command.mailbox_id.clone());
        }
        self.replace_mailboxes(
            account_id,
            message_id,
            &ReplaceMailboxesCommand { mailbox_ids },
        )
        .await
    }

    /// Remove a message from a mailbox (idempotent: no-op if absent).
    ///
    /// @spec docs/L1-api#message-commands
    pub async fn remove_from_mailbox(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &RemoveFromMailboxCommand,
    ) -> Result<CommandAck, ServiceError> {
        let mailbox_ids: Vec<MailboxId> = self
            .list_message_mailboxes_with_overlay(account_id, message_id)?
            .into_iter()
            .filter(|id| id != &command.mailbox_id)
            .collect();
        self.replace_mailboxes(
            account_id,
            message_id,
            &ReplaceMailboxesCommand { mailbox_ids },
        )
        .await
    }

    /// Permanently delete a message, local-first.
    ///
    /// @spec docs/L1-api#message-commands
    /// @spec docs/L1-outbox#operation-model
    pub async fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<CommandAck, ServiceError> {
        self.queue_then_emit_message_operation(
            account_id,
            message_id,
            OperationKind::Destroy,
            serde_json::json!({}),
        )
        .await
    }

    fn list_message_mailboxes_with_overlay(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Vec<MailboxId>, ServiceError> {
        // Body-free: this needs only mailbox membership. The summary read
        // serves the EFFECTIVE plane (base ∪ overlay), so pending folds are
        // already included — no separate overlay pass.
        let Some(summary) = self
            .message_detail_reader
            .get_message_summary(account_id, message_id)?
        else {
            return Err(ServiceError::from(StoreError::NotFound(format!(
                "message:{}",
                message_id.as_str()
            ))));
        };
        Ok(summary.mailbox_ids)
    }
}
