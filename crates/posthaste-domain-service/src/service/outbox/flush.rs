//! The flush drain: [`MailService::flush_account`] loops `flush_pass` over the
//! flushable operations, routing each push outcome (settle / re-queue / park /
//! fail). Ordering is the insertion-order drain — there are no cross-operation
//! dependency edges (D174).

use super::classify::{FlushDisposition, FlushError};
use super::push::Pushed;
use crate::service::*;

/// BE-H2: after this many consecutive transient failures an operation stops
/// halting the drain — it is skipped (still pending, retried each pass) so a
/// poisoned "transient" op cannot wedge the account's outbox behind it.
const TRANSIENT_STOP_THRESHOLD: u32 = 3;

impl MailService {
    /// Flush all flushable operations for an account to the provider, returning
    /// the `operation.settled` events to publish.
    ///
    /// Stops draining on the first transient (offline) failure so later ops are
    /// retried together on the next connectivity window. Per-entity ordering is
    /// the insertion-order drain (D174 — no dependency edges).
    ///
    /// Draft consumption is GATEWAY-OWNED (NS2 Slice 4): a consuming send
    /// destroys its draft inside its own provider execution, so no settlement
    /// fan-out op exists and one pass drains everything queued.
    ///
    /// @spec docs/L1-outbox#state-machine
    pub async fn flush_account(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let mut events = Vec::new();
        self.flush_pass(account_id, gateway, &mut events).await?;
        Ok(events)
    }

    /// One drain pass over the currently-flushable operations, appending the
    /// settlement events to `events`. Returns early (without error) when the
    /// drain stops on a transient/uncertain failure — the rest retries on the
    /// next connectivity window.
    async fn flush_pass(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
        events: &mut Vec<DomainEvent>,
    ) -> Result<(), ServiceError> {
        // Held sends: two readiness gates on two clocks (D152) — send-later
        // (`send_at`) against a RE-SAMPLED wall clock; undo holds
        // (`hold_until_mono`) against the same monotonic anchor that stamped
        // them. Sampled once per pass; a send coming due mid-pass waits for
        // the next pass/tick, which only ever delays, never fires early.
        let wall_now = super::schedule::wall_now_rfc3339()
            .map_err(|error| ServiceError::from(GatewayError::Rejected(error)))?;
        let mono_now = super::schedule::monotonic_now_secs();
        // D173 step 1: held sends' eager ensure-draft runs before the
        // readiness-gated drain (a held row is not flushable, but its draft
        // must reach the provider NOW for cross-device visibility).
        self.ensure_drafts_for_held_sends(account_id, gateway, &wall_now, mono_now, events)
            .await?;
        let queued = self
            .outbox
            .list_flushable_operations(account_id, &wall_now, mono_now)?;
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
            // The atomic flush gate (cancel-vs-flush, exactly one winner): claim
            // the op `inflight` with a single guarded conditional write. A user
            // cancel that already won removed the row, so the claim matches
            // nothing and the op is skipped — a discarded (undone) send is never
            // pushed. Once the claim wins, the discard path can no longer yank
            // the row (its DELETE is guarded on `state != 'inflight'`), so the
            // in-flight provider call is never orphaned either.
            if !self.outbox.claim_operation_for_flush(&operation.id)? {
                continue;
            }
            match self.push_operation(account_id, &operation, gateway).await {
                Ok(Pushed::Entity {
                    assigned_entity_id,
                    destroyed_entity_id,
                    send_filing,
                    cursor,
                }) => {
                    // The live id the draft's visible row is keyed under going
                    // INTO this settlement (pre-repoint) — the row a rotation
                    // must re-derive away from.
                    let old_live = if operation.entity.kind == OperationEntityKind::Draft {
                        self.draft_registry
                            .resolve_draft_entity(account_id, &operation.entity.id)?
                            .unwrap_or_else(|| operation.entity.id.clone())
                    } else {
                        operation.entity.id.clone()
                    };
                    if let Some(new_id) = assigned_entity_id.as_deref() {
                        if new_id != operation.entity.id {
                            // A draft op's entity id IS the stable key and never
                            // rotates, so a provider rotation is recorded as ONE
                            // registry write — key → live id. This IS the JMAP
                            // adoption bridge: the provider now holds the draft
                            // at `new_id`, and `op_row_touches` resolves the
                            // still-settled save's key to it, so replay
                            // materializes the derived row at the new id. Later
                            // ops carry the key too and resolve it fresh at
                            // their own flush; no outbox rewrite is needed.
                            self.draft_registry.set_draft_alias(
                                account_id,
                                &operation.entity.id,
                                new_id,
                            )?;
                        }
                    }
                    // The live id the draft's visible row is keyed under
                    // AFTER this settlement (post-repoint).
                    let new_live = assigned_entity_id
                        .as_deref()
                        .unwrap_or(old_live.as_str())
                        .to_string();
                    let settlement = OperationSettlement {
                        id: operation.id.clone(),
                        outcome: OperationOutcome::Applied,
                        assigned_entity_id,
                        error: None,
                        send_filing,
                    };
                    events.push(self.emit_settlement(account_id, &operation, &settlement)?);
                    if operation.kind == OperationKind::DraftDelete {
                        // Forget at SETTLEMENT — the provider has confirmed the
                        // destroy, so only now does the stable key stop naming a
                        // live draft (never at enqueue: an in-flight op must
                        // still resolve its mapping). Converges with the
                        // sync-observed forget (the confirmed-gone prune): both
                        // are idempotent deletes of the same registry row, so
                        // whichever observes the confirmed destruction second is
                        // a no-op — the mapping is forgotten exactly once, on
                        // confirmed destruction.
                        self.draft_registry
                            .remove_draft_alias(account_id, &operation.entity.id)?;
                        // A settled DraftDelete emits the reconciling
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
                    if operation.kind.is_draft_save() || operation.kind == OperationKind::Send {
                        // A CONTENT op (putDraft / send): its authored row is
                        // DERIVED from the op payload by replay, so the op does
                        // not leave the log at settlement — it rests in the
                        // `applied` state, still folded, until causal
                        // truncation. The row therefore never blinks out
                        // between settlement and the next sync, and it cannot
                        // outlive its op. The truncation watermark is the
                        // provider sync position the save/send returned (JMAP
                        // `newState`) when the gateway exposes one; otherwise
                        // the cycle rule retires it, one sync cycle later.
                        let watermark = cursor
                            .as_ref()
                            .filter(|cursor| cursor.object_type == SyncObject::Message)
                            .map(|cursor| cursor.state.as_str());
                        self.outbox.mark_operation_settled(
                            &operation.id,
                            super::schedule::monotonic_now_secs(),
                            watermark,
                        )?;
                        // Re-derive the row at both ends of a rotation: the
                        // pre-repoint id (the fold no longer lands there, so it
                        // retires — with a prune echo) and the post-repoint id
                        // (the settled op still folds, so the derived row
                        // materializes there — with a projection echo). A send
                        // (no rotation) re-derives its one Sent row.
                        self.settle_content_op_overlay(
                            account_id, &operation, &old_live, &new_live, events,
                        )
                        .await?;
                    } else {
                        // An intent-ish destroy (DraftDelete): its base-write
                        // reconcile already settled it, so it leaves the log now.
                        self.outbox.remove_operation(&operation.id)?;
                    }
                }
                Ok(Pushed::Message {
                    readback,
                    rejected,
                    cursor,
                }) => {
                    // Settle from the provider outcome: with a readback (or a
                    // rejection) the op leaves the log now — the settle write
                    // makes base authoritative; a blind settlement rests the
                    // op in the log (state `applied`, still replayed) until
                    // causal truncation, recording the returned sync position
                    // as its watermark.
                    events.extend(
                        self.settle_message_operation(
                            account_id, &operation, readback, rejected, cursor,
                        )
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
                    let attempts = operation.attempts + 1;
                    self.outbox.update_operation_state(
                        &operation.id,
                        OperationState::Pending,
                        attempts,
                        Some(&message),
                    )?;
                    // BE-H2 head-of-line guard: the first few transient
                    // failures stop the drain (the offline-friendly default —
                    // everything retries on the next connectivity window). An
                    // op that keeps failing past the threshold is POISONED-OR-
                    // UNLUCKY and no longer gets to wedge the account: it is
                    // SKIPPED (still pending, still retried each pass, still
                    // cancelable) and the drain continues to the ops behind
                    // it. Deliberately no permanent quarantine: a real offline
                    // stretch also accumulates attempts, and failing a user's
                    // legitimate op for being offline too long would be worse
                    // than one extra no-op provider call per flush window.
                    if attempts < TRANSIENT_STOP_THRESHOLD {
                        // Offline: stop draining; the rest retries next window.
                        return Ok(());
                    }
                    ph_warn!(
                        events::OUTBOX_TRANSIENT_OP_SKIPPED,
                        account_id = %account_id,
                        operation_id = %operation.id,
                        attempts,
                        error = %message,
                        "transient-failing op past threshold; skipping so it \
                         cannot wedge the outbox (BE-H2)"
                    );
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
                    // Re-derive the rows the op touched over the log that now
                    // holds it as DispatchUncertain. A parked send is a content
                    // op whose words are never dropped: it keeps folding (see
                    // `is_replayable`) — DispatchUncertain is not Pending, so the
                    // fold is NOT held and reproduces its provisional Sent row,
                    // visible as needs-attention until the user retries or
                    // discards it. Without this re-derive the overlay would lag
                    // the state change.
                    for row_id in self.op_touched_row_ids(account_id, &operation)? {
                        self.refresh_message_overlay(account_id, &row_id).await?;
                    }
                    // A send timeout signals a struggling link; stop draining so
                    // the rest retries on the next connectivity window.
                    return Ok(());
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
                        send_filing: None,
                    };
                    events.push(self.emit_settlement(account_id, &operation, &settlement)?);
                    if let Some(correction) =
                        self.emit_failure_base_correction(account_id, &operation)?
                    {
                        events.push(correction);
                    }
                    // Re-derive the rows the op touched over the log that now
                    // holds it as Failed. A CONTENT op (putDraft / send) stays
                    // PARKED with its authored row visible — a failed content
                    // op keeps folding (see `is_replayable`), so the re-derive
                    // reproduces the same row; words are never silently
                    // dropped, and the user keeps or discards it from the
                    // outbox. An INTENT op is speculation that lost: it folds
                    // nothing while Failed, so the re-derive reverts its row to
                    // base at once.
                    for row_id in self.op_touched_row_ids(account_id, &operation)? {
                        self.refresh_message_overlay(account_id, &row_id).await?;
                    }
                }
            }
        }
        Ok(())
    }
}
