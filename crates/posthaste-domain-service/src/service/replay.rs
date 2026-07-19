//! The replay engine: visible mail state is `replay(log, base)`.
//!
//! The overlay plane's override rows are DERIVED — folded from the unsettled
//! intent ops over the sync-owned base rows — and are recomputed whenever an
//! input changes: a log write (command accepted, op settled, op removed) or a
//! base write (each applied sync chunk, plus the post-sync sweep).
//! [`MailService::refresh_message_overlay`] is the incremental unit (one
//! row); [`MailService::replay_account_overrides`] is the full rebuild from
//! (log, base) — always legal, and the recovery path that reproduces every
//! DERIVED override row of a wiped view.
//!
//! Pinned rows (a `draft_id`-carrying draft row or a `phsend-` provisional
//! Sent row with no base row) are not override rows: their owning op has
//! already settled, so they are not derivable from (log, base) and a wipe
//! loses them. Replay passes them through unchanged wherever they exist (the
//! keep-pins arm of the retire check).

use posthaste_domain_model::{
    AccountId, MailboxId, MessageId, MessageRecord, Operation, OperationEntityKind, OperationKind,
    OperationState, SendMessageRequest, ServiceError, StoreError, ThreadId,
};

use super::message_queries::{intent_fold_effect, project_record, FoldEffect};
use super::{offload, MailService};

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

/// The role a visible row plays for an entity-intent op in the fold: draft
/// ops address their key's live row; a send addresses TWO rows — its own
/// provisional Sent row and the consumed draft's live row.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EntityFoldRole {
    /// The row a draft save/discard's stable key currently maps to.
    DraftKey,
    /// The send op's own entity id: the provisional Sent row.
    SendRow,
    /// The live row of the draft a send consumes.
    SendConsumedDraft,
}

/// How an op addresses one visible row: as a message state assertion replayed
/// by the shared predictor, or as an entity-plane fold effect.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OpRowRole {
    Assertion,
    Entity(EntityFoldRole),
}

/// Synthesize the provisional Sent overlay row a due/dispatched send folds
/// to: visible in Sent from dispatch, adopted (retired) once
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

/// Synthesize the overlay row a queued draft save folds to: the
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

/// The op states whose effects the replay folds: everything not yet retired
/// by settlement coverage (a rejected/failed op folds nothing — base wins).
fn is_replayable(op: &Operation) -> bool {
    matches!(
        op.state,
        OperationState::Pending | OperationState::Inflight | OperationState::Applied
    )
}

/// A pinned overlay row — a settled draft save awaiting its provider copy
/// (`draft_id` set) or a provisional Sent row awaiting adoption (`phsend-`
/// identity). Not derivable from the log; replay passes it through unchanged.
fn is_pinned_row(record: &MessageRecord) -> bool {
    record.draft_id.is_some()
        || record
            .rfc_message_id
            .as_deref()
            .is_some_and(|rfc| rfc.starts_with("phsend-"))
}

impl MailService {
    /// Incremental replay of one visible row: re-derive its OVERLAY entry
    /// from base + its unsettled intents. The single maintenance function for
    /// the derived plane, called at every lifecycle moment that changes the
    /// replay's inputs — mutation queue, draft save/discard queue, op
    /// settlement, and the post-sync sweep.
    ///
    /// `message_id` is the row's LIVE id: for message assertions the op's
    /// entity id; for draft intents the registry-resolved live id their stable
    /// key currently maps to (draft ops carry the KEY, the overlay is keyed by
    /// the visible row).
    ///
    /// No unsettled ops → the entry is removed (base shows through), subject
    /// to `retire`. Folded-to-removed → tombstone. A discard folding over no
    /// base row removes the entry (nothing to hide). No base row under plain
    /// assertions → the entry is removed too (a pending flag folds over
    /// nothing once a remote delete wins; only a pending Destroy keeps its
    /// tombstone), so the visible row stays a pure function of (log, base).
    /// Pinned rows pass through unchanged in every arm.
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
        // with the ROLE this row plays for them (a send is multi-row — its
        // own provisional Sent row AND the consumed draft's live row).
        // Relevance comes from the shared op→row mapping
        // (`op_row_touches`), the same mapping the rebuild inventory inverts.
        let mut entity_ops: Vec<(Operation, EntityFoldRole)> = Vec::new();
        for op in unsettled {
            if !is_replayable(&op) {
                continue;
            }
            for (row_id, role) in self.op_row_touches(account_id, &op)? {
                if row_id != *message_id {
                    continue;
                }
                match role {
                    OpRowRole::Assertion => message_ops.push(op.clone()),
                    OpRowRole::Entity(entity_role) => entity_ops.push((op.clone(), entity_role)),
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
                            // Folded row but no base row. PINNED rows stay.
                            // Anything else is a GHOST — its op settled and
                            // base no longer holds the id, so the provider
                            // row either rotated (IMAP moves re-key the id;
                            // the new row is already in base) or was removed
                            // remotely; keeping it would serve a duplicate of
                            // the re-keyed row.
                            (Some(folded), None) => !is_pinned_row(&folded),
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
        // Entity-plane fold first (each effect is total), in insertion
        // order: the last queued save wins the row's content; a discard (or a
        // due send's consume) folds the row away; a due send upserts its
        // provisional Sent row. Message assertions then replay on top of
        // whatever row survives. The send fold is phase-aware: a HELD send
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
                        // A HELD send folds a DRAFT-FORM row — the hold's
                        // content is visible and cancelable even with no
                        // client-side save (the eager ensure step mirrors
                        // it provider-side).
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
                } else {
                    // Plain assertions over no base row fold to nothing: the
                    // remote removal wins and the entry is dropped, keeping
                    // the visible row derivable from (log, base) alone. A
                    // pinned row is not this fold's output — pass it through.
                    offload(move || {
                        match overlay.read_overlay_message(&account_id, &message_id)? {
                            Some(Some(folded)) if is_pinned_row(&folded) => Ok(()),
                            Some(_) => overlay.remove_overlay_message(&account_id, &message_id),
                            None => Ok(()),
                        }
                    })
                    .await?;
                }
            }
        }
        Ok(())
    }

    /// The visible row(s) one op addresses, with the role each row plays in
    /// the fold — the single op→row mapping. `refresh_message_overlay`
    /// selects the ops relevant to one row through it; the rebuild inventory
    /// inverts it (op → rows) so a full replay can enumerate every derived
    /// row from the log alone. Draft-key resolution goes through the one
    /// registry authority, key falling back to itself when unmapped.
    fn op_row_touches(
        &self,
        account_id: &AccountId,
        op: &Operation,
    ) -> Result<Vec<(MessageId, OpRowRole)>, ServiceError> {
        Ok(match op.entity.kind {
            OperationEntityKind::Message if op.kind == OperationKind::Send => {
                let mut touches = vec![(
                    MessageId::from(op.entity.id.as_str()),
                    OpRowRole::Entity(EntityFoldRole::SendRow),
                )];
                // A malformed send payload contributes no consumed-draft
                // touch; its own row still errors visibly in the fold.
                if let Ok(Some(FoldEffect::SendEffects {
                    consumes_draft_key: Some(key),
                    ..
                })) = intent_fold_effect(op)
                {
                    let consumed_live = self
                        .draft_registry
                        .resolve_draft_entity(account_id, &key)?
                        .unwrap_or(key);
                    if consumed_live != op.entity.id {
                        touches.push((
                            MessageId::from(consumed_live.as_str()),
                            OpRowRole::Entity(EntityFoldRole::SendConsumedDraft),
                        ));
                    }
                }
                touches
            }
            OperationEntityKind::Message if op.kind.is_state_assertion() => {
                vec![(MessageId::from(op.entity.id.as_str()), OpRowRole::Assertion)]
            }
            OperationEntityKind::Message => Vec::new(),
            OperationEntityKind::Draft => {
                let live = self
                    .draft_registry
                    .resolve_draft_entity(account_id, &op.entity.id)?
                    .unwrap_or_else(|| op.entity.id.clone());
                vec![(
                    MessageId::from(live.as_str()),
                    OpRowRole::Entity(EntityFoldRole::DraftKey),
                )]
            }
        })
    }

    /// The live row ids one op's replay effect touches — the incremental
    /// replay key, and the log side of the rebuild inventory.
    pub(crate) fn op_touched_row_ids(
        &self,
        account_id: &AccountId,
        op: &Operation,
    ) -> Result<Vec<MessageId>, ServiceError> {
        Ok(self
            .op_row_touches(account_id, op)?
            .into_iter()
            .map(|(row_id, _)| row_id)
            .collect())
    }

    /// Every row id replay must visit for an account: the union of the rows
    /// the unsettled log touches (so a wiped derived row reappears from the
    /// log alone) and the rows currently overlaid (so all-settled entries get
    /// their retire pass and pinned rows are passed through unchanged).
    pub(crate) async fn replay_inventory(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<MessageId>, ServiceError> {
        let unsettled = {
            let outbox = self.outbox.clone();
            let account_id = account_id.clone();
            offload(move || outbox.list_unsettled_operations(&account_id)).await?
        };
        let mut inventory = std::collections::BTreeSet::new();
        for op in unsettled.iter().filter(|op| is_replayable(op)) {
            inventory.extend(self.op_touched_row_ids(account_id, op)?);
        }
        let overlay_ids = {
            let overlay = self.overlay.clone();
            let account_id = account_id.clone();
            offload(move || overlay.list_overlay_message_ids(&account_id)).await?
        };
        inventory.extend(overlay_ids);
        Ok(inventory.into_iter().collect())
    }

    /// Full rebuild of the account's derived override rows from (log, base):
    /// replay every row in the inventory with the retire-on-confirmation
    /// policy. Always legal — a wiped derived row reappears because the
    /// inventory is log-derived, not overlay-derived — and idempotent; the
    /// recovery path for a lost or distrusted derived view.
    pub async fn replay_account_overrides(
        &self,
        account_id: &AccountId,
    ) -> Result<(), ServiceError> {
        for message_id in self.replay_inventory(account_id).await? {
            self.refresh_message_overlay(
                account_id,
                &message_id,
                OverlayRetire::ConfirmAgainstBase,
            )
            .await?;
        }
        Ok(())
    }

    /// Incremental replay for one sync base write: re-derive every written
    /// row the replay inventory covers, before the write's events are
    /// published. This is the per-base-write trigger — it runs per applied
    /// chunk (and per reconcile prune), so a pending op's fold never serves
    /// or broadcasts a snapshot of the OLD base, even when the sync cycle
    /// later aborts and the end-of-cycle sweep never runs.
    pub(crate) async fn replay_base_write(
        &self,
        account_id: &AccountId,
        written_rows: &std::collections::BTreeSet<MessageId>,
    ) -> Result<(), ServiceError> {
        if written_rows.is_empty() {
            return Ok(());
        }
        for message_id in self.replay_inventory(account_id).await? {
            if !written_rows.contains(&message_id) {
                continue;
            }
            self.refresh_message_overlay(
                account_id,
                &message_id,
                OverlayRetire::ConfirmAgainstBase,
            )
            .await?;
        }
        Ok(())
    }
}
