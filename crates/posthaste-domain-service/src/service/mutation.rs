use std::sync::Arc;

use posthaste_domain_model::{
    AccountId, AddToMailboxCommand, CommandAck, DomainEvent, MailboxId, MessageChangeAssertion,
    MessageId, Operation, OperationEntity, OperationEntityKind, OperationKind, OperationState,
    RemoveFromMailboxCommand, ReplaceMailboxesCommand, ServiceError, SetKeywordsCommand,
    StoreError, EVENT_TOPIC_MESSAGE_UPDATED,
};
use serde_json::json;

use super::message_queries::project_record;
use super::{decode_payload, encode_payload, offload, MailService};
use crate::{MessageOverlayStore, OperationOutboxStore};

/// Re-derive one message's OVERLAY entry from base + its unsettled state
/// assertions (NS1 cutover, RFC-L2-client-replication-model D167): the single
/// maintenance function for the optimistic plane, called at every lifecycle
/// moment that can change the fold's inputs — mutation queue, op settlement,
/// and a sync batch touching an overlaid id.
///
/// No unsettled ops → the entry is removed (base shows through).
/// Folded-to-removed → tombstone. No base row with a pending Destroy →
/// tombstone (hide the last-known row). No base row otherwise → any existing
/// entry stays as the last-known fold (e.g. a pending flag racing a remote
/// delete) until its op settles and this runs again.
///
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

/// An associated fn over cloned `Arc`s (not `&self`) so the sync sink can
/// call it without holding the service.
pub(crate) async fn refresh_message_overlay(
    overlay: Arc<dyn MessageOverlayStore>,
    outbox: Arc<dyn OperationOutboxStore>,
    account_id: AccountId,
    message_id: MessageId,
    retire: OverlayRetire,
) -> Result<(), ServiceError> {
    let unsettled = {
        let outbox = outbox.clone();
        let account_id = account_id.clone();
        offload(move || outbox.list_unsettled_operations(&account_id)).await?
    };
    let ops_for_message: Vec<Operation> = unsettled
        .into_iter()
        .filter(|op| {
            op.entity.kind == OperationEntityKind::Message
                && op.kind.is_state_assertion()
                && matches!(
                    op.state,
                    OperationState::Pending | OperationState::Inflight | OperationState::Applied
                )
                && op.entity.id == message_id.as_str()
        })
        .collect();
    if ops_for_message.is_empty() {
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
                        // keyword + mailbox sets.
                        (Some(mut folded), Some(mut base)) => {
                            folded.keywords.sort();
                            base.keywords.sort();
                            folded.mailbox_ids.sort();
                            base.mailbox_ids.sort();
                            folded.keywords == base.keywords
                                && folded.mailbox_ids == base.mailbox_ids
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
        offload(move || overlay.remove_overlay_message(&account_id, &message_id)).await?;
        return Ok(());
    }
    let base = {
        let overlay = overlay.clone();
        let account_id = account_id.clone();
        let message_id = message_id.clone();
        offload(move || overlay.read_base_message_record(&account_id, &message_id)).await?
    };
    match base {
        Some(record) => match project_record(record, &ops_for_message)? {
            Some(folded) => {
                offload(move || overlay.upsert_overlay_message(&account_id, &folded)).await?;
            }
            None => {
                offload(move || overlay.tombstone_overlay_message(&account_id, &message_id))
                    .await?;
            }
        },
        None => {
            if ops_for_message
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

impl MailService {
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

        refresh_message_overlay(
            self.overlay.clone(),
            self.outbox.clone(),
            account_id.clone(),
            message_id.clone(),
            // Ops are non-empty here (one was just queued), so no retire
            // decision arises.
            OverlayRetire::Immediate,
        )
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
                let command: ReplaceMailboxesCommand =
                    decode_payload(operation.payload.clone(), "replaceMailboxes payload")?;
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
