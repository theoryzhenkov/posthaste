use posthaste_domain_model::{
    AccountId, AddToMailboxCommand, CommandAck, DomainEvent, MailboxId, MessageId, Operation,
    OperationEntity, OperationEntityKind, OperationKind, RemoveFromMailboxCommand,
    ReplaceMailboxesCommand, ServiceError, SetKeywordsCommand, StoreError,
};

use super::{decode_payload, encode_payload, offload, MailService};

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

    async fn queue_then_emit_message_operation(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        kind: OperationKind,
        payload: serde_json::Value,
    ) -> Result<CommandAck, ServiceError> {
        // Echo the store command's ENRICHED `message.updated` (projection +
        // absolute `countDeltas`) instead of a bare `{changes:{keywords:true}}`
        // one. The write-through store command
        // (`set_keywords`/`replace_mailboxes`/`destroy_message`, `posthaste-store`
        // `mutations/commands.rs`) already computes and appends that event to the
        // log inside its transaction and hands it back in the `CommandResult`;
        // we now publish it rather than discarding it. This is the SAME event
        // shape the sync-apply path emits, so the client entity store ingests the
        // echo identically → the source-mailbox live count moves on the echo
        // (sub-second), not only when a later sync re-emits the countDeltas.
        //
        // No double-count: `countDeltas` carry ABSOLUTE mailbox counts (the
        // current row value, not a ±delta — `mailbox_counts_json_tx`), and the
        // client applies them by assignment (`apply_count_delta`), so this echo
        // and the follow-up sync's re-emitted event are idempotent — both set the
        // same value. Revert on failure rides the existing settlement path: a
        // rejected assertion settles from the provider readback, which rewrites
        // canonical to the unchanged state and re-emits an enriched
        // `message.updated` with the reverted absolute counts.
        //
        // The projection is the body-free `MessageSummary` (no HTML/text body),
        // so the settlement payload stays small — regression-gated by
        // `message_mutation_settlement_payload_excludes_the_message_body`.
        //
        // On a local write failure, retract the op so the outbox and canonical do
        // not diverge.
        //
        // @spec docs/eph/DESIGN-L2-optimistic-projection#3-the-runtime-write-through-mechanics
        let operation = self.queue_message_operation(account_id, message_id, kind, payload)?;
        let events = match self
            .apply_assertion_to_canonical(account_id, message_id, &operation)
            .await
        {
            Ok(events) => events,
            Err(error) => {
                return Err(self.remove_operation_after_local_failure(&operation, error));
            }
        };
        Ok(CommandAck { events })
    }

    /// Apply a message assertion's effect to the canonical row (optimistic
    /// write-through), deserializing the operation payload by kind, and return
    /// the store command's enriched `message.updated` events (projection +
    /// absolute `countDeltas`) so the caller can echo them to clients. Reuses the
    /// `MessageCommandStore` local-write methods; the `CommandResult` those
    /// return already carries the enriched event, so the optimistic echo and the
    /// sync-apply path share one event shape.
    ///
    /// @spec docs/eph/DESIGN-L2-optimistic-projection#3-the-runtime-write-through-mechanics
    async fn apply_assertion_to_canonical(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        operation: &Operation,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let result = match operation.kind {
            OperationKind::SetKeywords => {
                let command: SetKeywordsCommand =
                    decode_payload(operation.payload.clone(), "setKeywords payload")?;
                let message_commands = self.message_commands.clone();
                let owned_account_id = account_id.clone();
                let owned_message_id = message_id.clone();
                offload(move || {
                    message_commands.set_keywords(&owned_account_id, &owned_message_id, None, &command)
                })
                .await?
            }
            OperationKind::ReplaceMailboxes => {
                let command: ReplaceMailboxesCommand =
                    decode_payload(operation.payload.clone(), "replaceMailboxes payload")?;
                let message_commands = self.message_commands.clone();
                let owned_account_id = account_id.clone();
                let owned_message_id = message_id.clone();
                offload(move || {
                    message_commands.replace_mailboxes(
                        &owned_account_id,
                        &owned_message_id,
                        None,
                        &command,
                    )
                })
                .await?
            }
            OperationKind::Destroy => {
                let message_commands = self.message_commands.clone();
                let owned_account_id = account_id.clone();
                let owned_message_id = message_id.clone();
                offload(move || {
                    message_commands.destroy_message(&owned_account_id, &owned_message_id, None)
                })
                .await?
            }
            _ => return Ok(Vec::new()),
        };
        Ok(result.events)
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
