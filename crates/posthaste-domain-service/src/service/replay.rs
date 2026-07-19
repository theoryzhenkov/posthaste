//! The replay engine: visible mail state is `replay(log, base)`.
//!
//! The overlay plane's override rows are DERIVED — folded from the log's
//! replayable ops over the sync-owned base rows — and are recomputed whenever
//! an input changes: a log write (command accepted, op settled, op truncated)
//! or a base write (each applied sync chunk, plus the post-sync sweep). The
//! log includes SETTLED message assertions: a blind-settled op (no provider
//! readback) rests in the `applied` state, still folded so its effect keeps
//! serving until base catches up, and leaves the log only by CAUSAL
//! truncation ([`MailService::truncate_settled_operations`]) — never by
//! comparing base state against a fold.
//! [`MailService::refresh_message_overlay`] is the incremental unit (one
//! row); [`MailService::replay_account_overrides`] is the full rebuild from
//! (log, base) — always legal, and the recovery path that reproduces EVERY row
//! of a wiped view. Nothing is a non-derivable pass-through: an intent op's
//! override row folds over base, and a CONTENT op's authored row (a draft's
//! visible row, a send's provisional Sent row) is materialized from the op
//! payload by the same fold. A row that predates the provider cannot outlive
//! its op — wipe the view, replay, and every draft and provisional-Sent row
//! reappears from the content ops still in the log.

use posthaste_domain_model::{
    AccountId, DomainEvent, MailboxId, MessageId, MessageRecord, Operation, OperationEntityKind,
    OperationKind, OperationState, SendMessageRequest, ServiceError, StoreError, SyncObject,
    ThreadId, EVENT_TOPIC_MESSAGE_UPDATED,
};

use super::message_queries::{intent_fold_effect, project_record, FoldEffect};
use super::{offload, DeriveDiff, DeriveSnapshot, MailService, OverlayMutation};

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

/// The derived visible row of a send content op: the provisional Sent row,
/// materialized purely from the op payload. Visible in Sent from dispatch; it
/// leaves the log by causal truncation once adoption records the provider copy
/// (matched by the transport-shared `Message-ID` prefix,
/// [`posthaste_domain_model::send_identity_prefix`]), at which point base
/// serves the row. The domain half of the stamped id is a best-effort guess
/// from the sender (adoption ignores it).
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

/// The derived visible row of a draft save content op, materialized purely
/// from the op payload: the draft is VISIBLE the moment its op is queued — no
/// provider round trip, no sync lag — and stays derived while the op rests
/// settled (`applied`), leaving the log by causal truncation once its provider
/// copy is in base. Body fields stay `None` (the overlay never carries bodies);
/// the op payload is the content authority until the save's provider copy
/// serves it (`get_draft_content` reads it back from the op payload).
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
        // `$seen` matches the gateways' save semantics (an own draft is
        // never "new mail"; IMAP itself appends `\Draft` only).
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

/// The op states whose effects the replay folds.
///
/// Pending and inflight ops fold (optimism in flight); settled (`applied`) ops
/// fold too, awaiting causal truncation — their effect keeps serving until base
/// catches up.
///
/// The Failed/DispatchUncertain boundary splits by op class. An INTENT op
/// (flag/move/delete) is speculation: when it fails or parks it folds nothing
/// and base wins. A CONTENT op (`putDraft`/`send`) carries authored mail that
/// is never dropped: a permanently failed or dispatch-uncertain content op
/// stays PARKED with its derived row still visible, so the user can keep or
/// discard it from the outbox. Discard (removing the op) is the only thing that
/// makes a content row vanish.
fn is_replayable(op: &Operation) -> bool {
    match op.state {
        OperationState::Pending | OperationState::Inflight | OperationState::Applied => true,
        OperationState::Failed | OperationState::DispatchUncertain => is_content_op(op),
    }
}

/// A content op carries authored mail — a draft save or a send. Its derived row
/// is never speculation: it survives failure/park, and leaves only when the op
/// is discarded or causally truncated. Delegates to
/// [`OperationKind::is_content_op`] — the single source shared with the
/// store's fold SQL filter.
fn is_content_op(op: &Operation) -> bool {
    op.kind.is_content_op()
}

/// Map a service-layer error to a store error for the fold's `StoreError`
/// return. Decode/internal faults become `Failure`; a wrapped `StoreError` is
/// unwrapped losslessly.
fn to_store(error: ServiceError) -> StoreError {
    match error {
        ServiceError::Store(store) => store,
        other => StoreError::Failure(other.to_string()),
    }
}

/// The op→row mapping, resolver-agnostic: the live id a draft op's stable key
/// maps to (and a send's consumed-draft key) come through `resolve` — the
/// registry authority for the inventory callers, the snapshot's pre-resolved
/// draft-key map for the atomic derive fold.
fn op_row_touches_with(
    op: &Operation,
    resolve: &dyn Fn(&str) -> Result<Option<String>, StoreError>,
) -> Result<Vec<(MessageId, OpRowRole)>, StoreError> {
    Ok(match op.entity.kind {
        OperationEntityKind::Message if op.kind == OperationKind::Send => {
            let mut touches = vec![(
                MessageId::from(op.entity.id.as_str()),
                OpRowRole::Entity(EntityFoldRole::SendRow),
            )];
            // A malformed send payload contributes no consumed-draft touch;
            // its own row still errors visibly in the fold.
            if let Ok(Some(FoldEffect::SendEffects {
                consumes_draft_key: Some(key),
                ..
            })) = intent_fold_effect(op)
            {
                let consumed_live = resolve(&key)?.unwrap_or(key);
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
            let live = resolve(&op.entity.id)?.unwrap_or_else(|| op.entity.id.clone());
            vec![(
                MessageId::from(live.as_str()),
                OpRowRole::Entity(EntityFoldRole::DraftKey),
            )]
        }
    })
}

/// The pure fold: one visible row as a function of the transaction-consistent
/// snapshot. Reads ONLY from `snapshot` — including the role-mailbox ids it
/// needs to file a draft or a provisional Sent row — and returns the overlay
/// mutation for the store to apply inside the same transaction. This is
/// `replay(log, base)` for one row — atomic, re-derivable, with no live-store
/// reads, no second connection, and no clock.
pub(crate) fn fold_overlay_row(
    snapshot: &DeriveSnapshot,
    message_id: &MessageId,
) -> Result<OverlayMutation, StoreError> {
    let resolve = |key: &str| Ok::<_, StoreError>(snapshot.draft_keys.get(key).cloned());
    let mut message_ops: Vec<Operation> = Vec::new();
    let mut entity_ops: Vec<(Operation, EntityFoldRole)> = Vec::new();
    for op in &snapshot.ops {
        if !is_replayable(op) {
            continue;
        }
        for (row_id, role) in op_row_touches_with(op, &resolve)? {
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
        // No replayable op touches the row: the entry derives from base
        // alone, so it is removed (base shows through) — except a TOMBSTONE
        // over a SURVIVING base row, which keeps hiding it.
        return Ok(match &snapshot.overlay {
            Some(None) if snapshot.base.is_some() => OverlayMutation::Keep,
            Some(_) => OverlayMutation::Remove,
            None => OverlayMutation::Keep,
        });
    }
    // Entity-plane fold first (each effect is total), in insertion order:
    // last queued save wins; a discard (or a due send's consume) folds the
    // row away; a due send upserts its provisional Sent row. Message
    // assertions replay on top of whatever row survives. The send fold is
    // phase-aware: a HELD send — `Pending`, never yet dispatched
    // (`attempts == 0`), with an undo hold or a send-later date — folds its
    // draft-form row (cancelable) and flips to the provisional Sent row only
    // once the flusher DISPATCHES it (Pending→Inflight). A transiently-failed
    // send returns to `Pending` with `attempts > 0`, so it stays Sent across
    // retries (the undo window is over once it has left). A no-hold send-now
    // is committed the moment it is queued, so it is never held. The
    // held→due transition is a LOG change (the dispatch / its retry state),
    // not a replay-time clock read — so the fold stays a pure function of
    // (log, base) and replaying twice yields the same rows.
    let send_is_held = |op: &Operation| {
        op.state == OperationState::Pending
            && op.attempts == 0
            && (op.send_at.is_some() || op.hold_until_mono.is_some())
    };
    let mut state = snapshot.base.clone();
    let mut discarded_by_draft_op = false;
    for (op, role) in &entity_ops {
        match (intent_fold_effect(op).map_err(to_store)?, role) {
            (Some(FoldEffect::UpsertDraft(request)), EntityFoldRole::DraftKey) => {
                state = Some(synthesize_draft_record(
                    state.take(),
                    &request,
                    op,
                    snapshot.drafts_mailbox.as_ref(),
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
                    state = Some(synthesize_sent_record(
                        &request,
                        op,
                        snapshot.sent_mailbox.as_ref(),
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
                    let key = consumes_draft_key.unwrap_or_default();
                    state = Some(synthesize_draft_record(
                        state.take(),
                        &request,
                        op,
                        snapshot.drafts_mailbox.as_ref(),
                        message_id,
                        &key,
                    ));
                    discarded_by_draft_op = false;
                } else {
                    state = None;
                    discarded_by_draft_op = true;
                }
            }
            _ => {}
        }
    }
    let base_exists = snapshot.base.is_some();
    Ok(match state {
        Some(record) => match project_record(record, &message_ops).map_err(to_store)? {
            Some(folded) => OverlayMutation::Upsert(Box::new(folded)),
            None => OverlayMutation::Tombstone,
        },
        None if discarded_by_draft_op => {
            if base_exists {
                OverlayMutation::Tombstone
            } else {
                OverlayMutation::Remove
            }
        }
        None => {
            if message_ops
                .iter()
                .any(|op| op.kind == OperationKind::Destroy)
            {
                OverlayMutation::Tombstone
            } else {
                // Plain assertions over no base row fold to nothing.
                match snapshot.overlay {
                    Some(_) => OverlayMutation::Remove,
                    None => OverlayMutation::Keep,
                }
            }
        }
    })
}

impl MailService {
    /// Incremental replay of one visible row: re-derive its OVERLAY entry
    /// from base + the replayable ops that touch it (pending, inflight, and
    /// settled-awaiting-truncation). The single maintenance function for the
    /// derived plane, called at every lifecycle moment that changes the
    /// replay's inputs — mutation queue, draft save/discard queue, op
    /// settlement, op truncation, and the post-sync sweep.
    ///
    /// `message_id` is the row's LIVE id: for message assertions the op's
    /// entity id; for draft intents the registry-resolved live id their stable
    /// key currently maps to (draft ops carry the KEY, the overlay is keyed by
    /// the visible row).
    ///
    /// No replayable ops → the entry derives from base alone, so it is
    /// removed (base shows through) — except a tombstone over a surviving base
    /// row, which keeps hiding it (the provider destroy has not synced out
    /// yet). Folded-to-removed → tombstone. A discard folding over no base row
    /// removes the entry (nothing to hide). No base row under plain assertions
    /// → the entry is removed too (a pending flag folds over nothing once a
    /// remote delete wins; only a pending Destroy keeps its tombstone), so the
    /// visible row stays a pure function of (log, base) — content rows
    /// included, since a content op that would materialize its row is itself a
    /// replayable op and takes the folding path below.
    pub(crate) async fn refresh_message_overlay(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<DeriveDiff, ServiceError> {
        // Atomic derive: one store transaction snapshots base + the unsettled
        // log + the draft-key map + the role mailboxes, runs the fold, and
        // applies the mutation. SQLite serializes writers, so no concurrent
        // base write (sync) or sibling refresh (another command) can
        // interleave to produce a stale overlay. The fold closure captures
        // only the row id — it reads everything else from the snapshot, so it
        // is a pure function of (log, base) with no live-store reads.
        let overlay = self.overlay.clone();
        let account_id = account_id.clone();
        let message_id = message_id.clone();
        let diff = offload(move || {
            overlay.derive_overlay(
                &account_id,
                &message_id,
                Box::new({
                    let message_id = message_id.clone();
                    move |snapshot| fold_overlay_row(snapshot, &message_id)
                }),
            )
        })
        .await?;
        Ok(diff)
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
        let registry = self.draft_registry.clone();
        let account_id = account_id.clone();
        let resolver = move |key: &str| registry.resolve_draft_entity(&account_id, key);
        op_row_touches_with(op, &resolver).map_err(ServiceError::from)
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
    /// the replayable log touches — settled-awaiting-truncation ops included,
    /// so a wiped derived row reappears from the log alone — and the rows
    /// currently overlaid (so entries whose ops truncated are removed).
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
    /// replay every row in the inventory. Always legal — a wiped derived row
    /// reappears because the inventory is log-derived, not overlay-derived,
    /// and the log's settled ops keep folding until truncation — and
    /// idempotent; the recovery path for a lost or distrusted derived view.
    pub async fn replay_account_overrides(
        &self,
        account_id: &AccountId,
    ) -> Result<(), ServiceError> {
        for message_id in self.replay_inventory(account_id).await? {
            self.refresh_message_overlay(account_id, &message_id)
                .await?;
        }
        Ok(())
    }

    /// Causal truncation: remove settled (`applied`) ops whose effect the
    /// sync chain has provably absorbed, then re-derive the rows they
    /// touched (base alone serves them from here). Two clocks, both pure
    /// ordering checks — no comparison of state decides anything:
    ///
    /// - WATERMARK (JMAP): settlement captured the provider sync position
    ///   that includes the change; the op truncates once the stored Message
    ///   cursor equals it. Opaque states have no comparable order, so this is
    ///   an equality fast path — a chain that jumps past the watermark
    ///   without ever committing it exactly falls back to the cycle rule.
    /// - CYCLE (IMAP; JMAP without a usable position): the op truncates once
    ///   a sync cycle that STARTED (strictly) after its settlement completes
    ///   — `cycle_started_mono` is the caller's cycle-entry stamp on the same
    ///   monotonic-anchored clock that stamped `settled_at_mono`. A legacy
    ///   row without a marker is truncate-eligible on any completed cycle.
    ///
    /// Runs only from a COMPLETED cycle's sweep: an aborted pull never
    /// truncates. If a provider consistency blip ever breaks the causal
    /// assumption, the next replay serves base truth and the following sync
    /// self-corrects — a one-cycle flicker, never durable wrong state.
    ///
    /// Returns the prune echoes a truncation produces: when a CONTENT op's
    /// derived row (a draft's local-key row, a provisional Sent row) retires
    /// and no row survives in its place, a `message.updated{deleted}` echo
    /// fires so the client drops the local/provisional id — the row's identity
    /// changed to the provider's, and refreshing the overlay alone emits no
    /// event.
    pub(crate) async fn truncate_settled_operations(
        &self,
        account_id: &AccountId,
        cycle_started_mono: i64,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let settled: Vec<posthaste_domain_model::SettledOperation> = {
            let outbox = self.outbox.clone();
            let account_id = account_id.clone();
            offload(move || outbox.list_settled_operations(&account_id)).await?
        };
        if settled.is_empty() {
            return Ok(Vec::new());
        }
        let message_cursor = self
            .sync_state
            .get_cursor(account_id, SyncObject::Message)?;
        let mut events = Vec::new();
        for settled_op in settled {
            let cycle_started_after_settlement =
                settled_op.settled_at_mono.unwrap_or(0) < cycle_started_mono;
            let watermark_reached = matches!(
                (&settled_op.watermark, &message_cursor),
                (Some(watermark), Some(cursor)) if *watermark == cursor.state
            );
            if !(cycle_started_after_settlement || watermark_reached) {
                continue;
            }
            let is_content = is_content_op(&settled_op.operation);
            let touched = self.op_touched_row_ids(account_id, &settled_op.operation)?;
            let account_id_owned = account_id.clone();
            // Atomic: remove the op and re-derive its touched rows in ONE
            // transaction, so a crash between them cannot leave a derived row
            // whose owning op is gone — the orphan the model claims is
            // unrepresentable. The per-row diff is the retire echo (no
            // separate before/after reads).
            let diffs = {
                let overlay = self.overlay.clone();
                let op_id = settled_op.operation.id.clone();
                let touched_owned = touched.clone();
                offload(move || {
                    overlay.remove_op_and_derive(
                        &account_id_owned,
                        &op_id,
                        &touched_owned,
                        Box::new(|row_id, snapshot| fold_overlay_row(snapshot, row_id)),
                    )
                })
                .await?
            };
            for (message_id, diff) in touched.iter().zip(diffs.into_iter()) {
                // A content op's derived row changes identity to the provider's
                // when truncated: the local/provisional id has no base row, so
                // once the derived entry retires echo `deleted` so the client
                // drops it.
                if is_content && diff.retired() {
                    events.push(self.events.append_event(
                        account_id,
                        EVENT_TOPIC_MESSAGE_UPDATED,
                        None,
                        Some(message_id),
                        serde_json::json!({ "messageId": message_id.as_str(), "deleted": true }),
                    )?);
                }
            }
        }
        Ok(events)
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
            self.refresh_message_overlay(account_id, &message_id)
                .await?;
        }
        Ok(())
    }
}
