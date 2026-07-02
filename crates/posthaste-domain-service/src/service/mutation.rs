use posthaste_domain_model::{
    AccountId, AddToMailboxCommand, CommandAck, MailboxId, MessageId, Operation, OperationEntity,
    OperationEntityKind, OperationKind, RemoveFromMailboxCommand, ReplaceMailboxesCommand,
    ServiceError, SetKeywordsCommand, StoreError, EVENT_TOPIC_MESSAGE_UPDATED,
};

use super::{decode_payload, encode_payload, MailService};

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

    fn queue_then_emit_message_operation(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        kind: OperationKind,
        payload: serde_json::Value,
        event_payload: serde_json::Value,
    ) -> Result<CommandAck, ServiceError> {
        // A state assertion acknowledges the change; it does not read or return
        // the message body. The synced mailbox memberships are both the
        // existence check (a known message has them) and the event's mailbox
        // hint; the body + attachments a full `get_message_detail` would load are
        // never needed here. Membership and counts propagate through the appended
        // event + server-side view recompute, which keys on the change flags (see
        // `event_affects_view`), not on this hint, and every caller discards
        // the command result for state assertions. Reading the body here made
        // archive/delete/keyword ops pay a load + serialize + transfer tax
        // proportional to body size on attachment-shaped messages — regression-
        // gated by `message_mutation_settlement_payload_excludes_the_message_body`.
        //
        // @spec docs/replication/client-link/L3#5-the-failure-path-and-remaining-gaps
        let projected_mailboxes = self
            .message_mailboxes
            .get_message_mailboxes(account_id, message_id)?;
        let operation = self.queue_message_operation(account_id, message_id, kind, payload)?;
        // Write-through: apply the assertion to the canonical row so SQLite
        // reflects the optimistic state directly. The read overlay still folds
        // the same assertion idempotently until S4, so reads are unchanged this
        // slice. On a local write failure, retract the op (as for an event-
        // append failure) so the outbox and canonical do not diverge.
        //
        // @spec docs/eph/DESIGN-L2-optimistic-projection#3-the-runtime-write-through-mechanics
        if let Err(error) = self.apply_assertion_to_canonical(account_id, message_id, &operation) {
            return Err(self.remove_operation_after_local_failure(&operation, error));
        }
        let event = match self.events.append_event(
            account_id,
            EVENT_TOPIC_MESSAGE_UPDATED,
            projected_mailboxes.first(),
            Some(message_id),
            event_payload,
        ) {
            Ok(event) => event,
            Err(error) => {
                return Err(self
                    .remove_operation_after_local_failure(&operation, ServiceError::from(error)));
            }
        };
        Ok(CommandAck {
            events: vec![event],
        })
    }

    /// Apply a message assertion's effect to the canonical row (optimistic
    /// write-through), deserializing the operation payload by kind. Reuses the
    /// `MessageCommandStore` local-write methods.
    ///
    /// @spec docs/eph/DESIGN-L2-optimistic-projection#3-the-runtime-write-through-mechanics
    fn apply_assertion_to_canonical(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        operation: &Operation,
    ) -> Result<(), ServiceError> {
        match operation.kind {
            OperationKind::SetKeywords => {
                let command: SetKeywordsCommand =
                    decode_payload(operation.payload.clone(), "setKeywords payload")?;
                self.message_commands
                    .set_keywords(account_id, message_id, None, &command)?;
            }
            OperationKind::ReplaceMailboxes => {
                let command: ReplaceMailboxesCommand =
                    decode_payload(operation.payload.clone(), "replaceMailboxes payload")?;
                self.message_commands
                    .replace_mailboxes(account_id, message_id, None, &command)?;
            }
            OperationKind::Destroy => {
                self.message_commands
                    .destroy_message(account_id, message_id, None)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Add/remove JMAP keywords on a message, local-first.
    ///
    /// Enqueues a state assertion and reflects it through the read-time overlay;
    /// the authoritative projection remains sync-owned.
    ///
    /// @spec docs/L1-api#message-commands
    /// @spec docs/L1-outbox#operation-model
    #[allow(clippy::unused_async)]
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
            serde_json::json!({
                "messageId": message_id.as_str(),
                "changes": { "keywords": true },
            }),
        )
    }

    /// Snooze scheduler: return every due snoozed message to the Inbox.
    /// Each move is the same `replace_mailboxes` write-through the client path
    /// uses, so the provider move is enqueued (flushed on the next sync) + the
    /// store invariant clears the snooze row immediately (no re-query next
    /// tick). Server-owned → cross-device coherent; not user-initiated → no
    /// undo step. Returns the count of messages returned.
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
    #[allow(clippy::unused_async)]
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
            serde_json::json!({
                "messageId": message_id.as_str(),
                "changes": { "mailboxes": true, "arrived": true },
                "mailboxIds": command
                    .mailbox_ids
                    .iter()
                    .map(MailboxId::as_str)
                    .collect::<Vec<_>>(),
                "arrivedMailboxIds": command
                    .mailbox_ids
                    .iter()
                    .map(MailboxId::as_str)
                    .collect::<Vec<_>>(),
            }),
        )
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
    #[allow(clippy::unused_async)]
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
            serde_json::json!({ "messageId": message_id.as_str(), "deleted": true }),
        )
    }

    fn list_message_mailboxes_with_overlay(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Vec<MailboxId>, ServiceError> {
        // Body-free: this needs only mailbox membership. Canonical holds the
        // optimistic membership (written through, S2), so read the summary
        // directly — no overlay fold.
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
