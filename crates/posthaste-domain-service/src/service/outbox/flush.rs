//! The flush drain: [`MailService::flush_account`] loops `flush_pass` over the
//! flushable operations, routing each push outcome (settle / re-queue / park /
//! fail) and honoring per-entity dependency ordering.

use super::classify::{FlushDisposition, FlushError};
use super::push::Pushed;
use crate::service::*;

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
        // Held sends: two readiness gates on two clocks (D152) — send-later
        // (`send_at`) against a RE-SAMPLED wall clock; undo holds
        // (`hold_until_mono`) against the same monotonic anchor that stamped
        // them. Sampled once per pass; a send coming due mid-pass waits for
        // the next pass/tick, which only ever delays, never fires early.
        let wall_now = super::schedule::wall_now_rfc3339()
            .map_err(|error| ServiceError::from(GatewayError::Rejected(error)))?;
        let mono_now = super::schedule::monotonic_now_secs();
        let queued = self
            .outbox
            .list_flushable_operations(account_id, &wall_now, mono_now)?;
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
                        return Ok(FlushPass {
                            stopped: true,
                            follow_up_enqueued,
                        });
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
                    continue;
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
}
