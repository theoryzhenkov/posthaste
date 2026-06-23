use crate::{
    AccountId, AddToMailboxCommand, CommandResult, GatewayError, MailboxId, MessageId,
    OperationEntity, OperationEntityKind, OperationKind, RemoveFromMailboxCommand,
    ReplaceMailboxesCommand, ServiceError, SetKeywordsCommand, StoreError,
    EVENT_TOPIC_MESSAGE_UPDATED,
};

use super::MailService;

impl MailService {
    fn queue_message_operation(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        kind: OperationKind,
        payload: serde_json::Value,
    ) -> Result<crate::Operation, ServiceError> {
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
        operation: &crate::Operation,
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
    ) -> Result<CommandResult, ServiceError> {
        // A state assertion acknowledges the change; it does not read or return
        // the message body. The synced mailbox memberships are both the
        // existence check (a known message has them) and the event's mailbox
        // hint; the body + attachments a full `get_message_detail` would load are
        // never needed here. Membership and counts propagate through the appended
        // event + server-side view recompute, which keys on the change flags (see
        // `event_affects_view`), not on this hint, and every caller discards
        // `CommandResult.detail` for state assertions. Reading the body here made
        // archive/delete/keyword ops pay a load + serialize + transfer tax
        // proportional to body size on attachment-shaped messages — regression-
        // gated by `message_mutation_settlement_payload_excludes_the_message_body`.
        //
        // @spec docs/replication/L3#7-hardening-w5-and-the-failure-path
        let projected_mailboxes = self
            .message_mailboxes
            .get_message_mailboxes(account_id, message_id)?;
        let operation = self.queue_message_operation(account_id, message_id, kind, payload)?;
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
        Ok(CommandResult {
            detail: None,
            events: vec![event],
        })
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
    ) -> Result<CommandResult, ServiceError> {
        let payload = serde_json::to_value(command).map_err(|error| {
            ServiceError::from(GatewayError::Rejected(format!(
                "failed to serialize keyword command: {error}"
            )))
        })?;
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
    ) -> Result<CommandResult, ServiceError> {
        let payload = serde_json::to_value(command).map_err(|error| {
            ServiceError::from(GatewayError::Rejected(format!(
                "failed to serialize mailbox command: {error}"
            )))
        })?;
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
    ) -> Result<CommandResult, ServiceError> {
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
    ) -> Result<CommandResult, ServiceError> {
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
    ) -> Result<CommandResult, ServiceError> {
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
        let Some(detail) = self
            .message_detail_reader
            .get_message_detail(account_id, message_id)?
            .and_then(|detail| self.apply_message_overlay(account_id, detail).transpose())
            .transpose()?
        else {
            return Err(ServiceError::from(StoreError::NotFound(format!(
                "message:{}",
                message_id.as_str()
            ))));
        };
        Ok(detail.summary.mailbox_ids)
    }
}
