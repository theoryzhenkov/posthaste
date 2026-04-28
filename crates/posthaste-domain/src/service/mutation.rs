use crate::{
    AccountId, AddToMailboxCommand, CommandResult, MailGateway, MessageId,
    RemoveFromMailboxCommand, ReplaceMailboxesCommand, ServiceError, SetKeywordsCommand,
    SyncObject,
};

use super::MailService;

/// Internal enum dispatching message mutations through a shared code path.
#[derive(Clone, Copy)]
enum MessageMutation<'a> {
    SetKeywords(&'a SetKeywordsCommand),
    ReplaceMailboxes(&'a ReplaceMailboxesCommand),
    Destroy,
}

impl MailService {
    /// Apply a message mutation: send to gateway with optimistic concurrency,
    /// then persist locally with the returned cursor.
    ///
    /// @spec docs/L1-sync#conflict-model
    async fn apply_message_mutation(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        mutation: MessageMutation<'_>,
        gateway: &dyn MailGateway,
    ) -> Result<CommandResult, ServiceError> {
        let expected_state = self
            .sync_state
            .get_cursor(account_id, SyncObject::Message)?;
        let outcome = match mutation {
            MessageMutation::SetKeywords(command) => {
                gateway
                    .set_keywords(
                        account_id,
                        message_id,
                        expected_state.as_ref().map(|cursor| cursor.state.as_str()),
                        command,
                    )
                    .await?
            }
            MessageMutation::ReplaceMailboxes(command) => {
                gateway
                    .replace_mailboxes(
                        account_id,
                        message_id,
                        expected_state.as_ref().map(|cursor| cursor.state.as_str()),
                        &command.mailbox_ids,
                    )
                    .await?
            }
            MessageMutation::Destroy => {
                gateway
                    .destroy_message(
                        account_id,
                        message_id,
                        expected_state.as_ref().map(|cursor| cursor.state.as_str()),
                    )
                    .await?
            }
        };

        match mutation {
            MessageMutation::SetKeywords(command) => self.message_commands.set_keywords(
                account_id,
                message_id,
                outcome.cursor.as_ref(),
                command,
            ),
            MessageMutation::ReplaceMailboxes(command) => self.message_commands.replace_mailboxes(
                account_id,
                message_id,
                outcome.cursor.as_ref(),
                command,
            ),
            MessageMutation::Destroy => self.message_commands.destroy_message(
                account_id,
                message_id,
                outcome.cursor.as_ref(),
            ),
        }
        .map_err(Into::into)
    }

    /// Add/remove JMAP keywords on a message.
    ///
    /// @spec docs/L1-api#message-commands
    pub async fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &SetKeywordsCommand,
        gateway: &dyn MailGateway,
    ) -> Result<CommandResult, ServiceError> {
        self.apply_message_mutation(
            account_id,
            message_id,
            MessageMutation::SetKeywords(command),
            gateway,
        )
        .await
    }

    /// Atomically replace all mailbox memberships for a message.
    ///
    /// @spec docs/L1-api#message-commands
    pub async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &ReplaceMailboxesCommand,
        gateway: &dyn MailGateway,
    ) -> Result<CommandResult, ServiceError> {
        self.apply_message_mutation(
            account_id,
            message_id,
            MessageMutation::ReplaceMailboxes(command),
            gateway,
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
        gateway: &dyn MailGateway,
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
            gateway,
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
        gateway: &dyn MailGateway,
    ) -> Result<CommandResult, ServiceError> {
        let mailbox_ids = self
            .message_mailboxes
            .get_message_mailboxes(account_id, message_id)?
            .into_iter()
            .filter(|mailbox_id| mailbox_id != &command.mailbox_id)
            .collect();
        self.replace_mailboxes(
            account_id,
            message_id,
            &ReplaceMailboxesCommand { mailbox_ids },
            gateway,
        )
        .await
    }

    /// Permanently delete a message.
    ///
    /// @spec docs/L1-api#message-commands
    pub async fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        gateway: &dyn MailGateway,
    ) -> Result<CommandResult, ServiceError> {
        self.apply_message_mutation(account_id, message_id, MessageMutation::Destroy, gateway)
            .await
    }
}
