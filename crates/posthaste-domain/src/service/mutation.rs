use crate::{
    AccountId, AddToMailboxCommand, CommandResult, GatewayError, MailboxId, MessageId,
    OperationEntity, OperationEntityKind, OperationKind, RemoveFromMailboxCommand,
    ReplaceMailboxesCommand, ServiceError, SetKeywordsCommand, SyncObject,
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
        let base_cursor = self
            .sync_state
            .get_cursor(account_id, SyncObject::Message)?
            .map(|cursor| cursor.provider_state());
        self.queue_operation(
            account_id,
            OperationEntity {
                kind: OperationEntityKind::Message,
                id: message_id.to_string(),
            },
            kind,
            payload,
            base_cursor,
        )
    }

    fn remove_operation_after_local_failure(
        &self,
        operation: &crate::Operation,
        error: crate::StoreError,
    ) -> ServiceError {
        let _ = self.outbox.remove_operation(&operation.id);
        error.into()
    }

    fn queue_then_apply_message_operation<F>(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        kind: OperationKind,
        payload: serde_json::Value,
        apply: F,
    ) -> Result<CommandResult, ServiceError>
    where
        F: FnOnce() -> Result<CommandResult, crate::StoreError>,
    {
        let operation = self.queue_message_operation(account_id, message_id, kind, payload)?;
        match apply() {
            Ok(result) => Ok(result),
            Err(error) => Err(self.remove_operation_after_local_failure(&operation, error)),
        }
    }

    /// Add/remove JMAP keywords on a message, local-first.
    ///
    /// Applies the local projection immediately and enqueues provider flush via
    /// the shared outbox. The returned `CommandResult` preserves the historical
    /// API/UI contract while `/operations` exposes the durable pending work.
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
        self.queue_then_apply_message_operation(
            account_id,
            message_id,
            OperationKind::SetKeywords,
            payload,
            || {
                self.message_commands
                    .set_keywords(account_id, message_id, None, command)
            },
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
        self.queue_then_apply_message_operation(
            account_id,
            message_id,
            OperationKind::ReplaceMailboxes,
            payload,
            || {
                self.message_commands
                    .replace_mailboxes(account_id, message_id, None, command)
            },
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
        let mut mailbox_ids = self
            .message_mailboxes
            .get_message_mailboxes(account_id, message_id)?;
        if !mailbox_ids
            .iter()
            .any(|mailbox_id| mailbox_id == &command.mailbox_id)
        {
            mailbox_ids.push(command.mailbox_id.clone());
        }
        self.replace_mailboxes(
            account_id,
            message_id,
            &ReplaceMailboxesCommand { mailbox_ids },
        )
        .await
    }

    /// Remove a message from a single mailbox.
    ///
    /// @spec docs/L1-api#message-commands
    pub async fn remove_from_mailbox(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &RemoveFromMailboxCommand,
    ) -> Result<CommandResult, ServiceError> {
        let mailbox_ids: Vec<MailboxId> = self
            .message_mailboxes
            .get_message_mailboxes(account_id, message_id)?
            .into_iter()
            .filter(|mailbox_id| mailbox_id != &command.mailbox_id)
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
        self.queue_then_apply_message_operation(
            account_id,
            message_id,
            OperationKind::Destroy,
            serde_json::json!({}),
            || {
                self.message_commands
                    .destroy_message(account_id, message_id, None)
            },
        )
    }
}
