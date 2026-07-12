//! Settlement: write the provider readback to base RAW (it is provider truth
//! arriving via the flush channel — the reconciler role, NS1 D161), re-derive
//! the message's overlay entry from the remaining unsettled ops, and emit the
//! `operation.settled` / `operation.dispatch_uncertain` / failure-correction
//! events.

use crate::service::*;
use posthaste_domain_model::{
    MessageReadback, MessageRecord, OperationDispatchUncertain, SyncBatch,
    EVENT_TOPIC_OPERATION_DISPATCH_UNCERTAIN,
};

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
    /// Settle a message state assertion from the provider readback: remove the
    /// op, write the RAW readback to base (provider truth — no optimism is
    /// folded into base, NS1), and re-derive the message's overlay entry from
    /// whatever ops remain unsettled. `Removed` deletes the base row; a `None`
    /// readback (a gateway that did not read back, e.g. IMAP) writes nothing —
    /// the overlay keeps serving the fold until a later sync reconciles base.
    ///
    /// @spec docs/eph/RFC-L2-client-replication-model#6-the-runtime-substrate-base--overlay--effective-d167d169
    pub(super) async fn settle_message_operation(
        &self,
        account_id: &AccountId,
        operation: &Operation,
        readback: Option<MessageReadback>,
        rejected: Option<String>,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let message_id = MessageId::from(operation.entity.id.as_str());
        // Remove the settled op FIRST so the overlay re-derivation below sees
        // only the ops that remain.
        self.outbox.remove_operation(&operation.id)?;
        let mut events = Vec::new();
        let had_readback = readback.is_some();
        let rejected_settlement = rejected.is_some();
        let batch = match readback {
            Some(MessageReadback::Present(record)) => Some(upsert_message_batch(record)),
            Some(MessageReadback::Removed) => Some(delete_message_batch(&message_id)),
            None => None,
        };
        if let Some(batch) = batch {
            let sync_writer = self.sync_writer.clone();
            let owned_account_id = account_id.clone();
            events.extend(
                offload(move || {
                    sync_writer.apply_sync_batch(
                        &BaseWrite::reconciler(),
                        &owned_account_id,
                        &batch,
                    )
                })
                .await?,
            );
        }
        // Overlay retire/refold. With a readback (or a rejection) base was just
        // made authoritative for this message → retire immediately (a rejection
        // reverts the optimism NOW). Without one (e.g. IMAP settles blind), the
        // settled effect has NOT reached base yet — retire only on confirmation
        // (the sweep removes the entry once a sync writes the effect into
        // base), so the row never flickers back between settle and sync.
        let retire = if had_readback || rejected_settlement {
            crate::service::mutation::OverlayRetire::Immediate
        } else {
            crate::service::mutation::OverlayRetire::ConfirmAgainstBase
        };
        self.refresh_message_overlay(account_id, &message_id, retire)
            .await?;
        let settlement = OperationSettlement {
            id: operation.id.clone(),
            outcome: if rejected.is_some() {
                OperationOutcome::Failed
            } else {
                OperationOutcome::Applied
            },
            assigned_entity_id: None,
            error: rejected,
            send_filing: None,
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
    pub(super) fn emit_failure_base_correction(
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
    pub(super) fn emit_dispatch_uncertain(
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

    pub(super) fn emit_settlement(
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
