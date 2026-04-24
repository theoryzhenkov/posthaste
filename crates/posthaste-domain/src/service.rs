use std::sync::Arc;

use serde_json::json;

use crate::{
    AccountId, AccountSettings, AddToMailboxCommand, AppSettings, CommandResult, ConfigDiff,
    ConfigRepository, ConversationCursor, ConversationId, ConversationPage, ConversationReadStore,
    ConversationSortField, ConversationView, EventStore, Identity, MailGateway, MailStore,
    MailboxId, MailboxReadStore, MailboxSummary, MessageCommandStore, MessageCursor,
    MessageDetailStore, MessageId, MessageListStore, MessageMailboxStore, MessagePage,
    MessageSortField, MessageSummary, RemoveFromMailboxCommand, ReplaceMailboxesCommand,
    SendMessageRequest, ServiceError, SetKeywordsCommand, SharedConfigRepository, SidebarResponse,
    SidebarSmartMailbox, SidebarSource, SmartMailbox, SmartMailboxId, SmartMailboxRule,
    SmartMailboxStore, SmartMailboxSummary, SortDirection, SourceDataStore, SourceProjectionStore,
    SyncObject, SyncStateStore, SyncTrigger, SyncWriteStore, ThreadId, ThreadView,
    EVENT_TOPIC_SYNC_COMPLETED, EVENT_TOPIC_SYNC_FAILED,
};
use crate::{DomainEvent, ServiceResultExt};

/// Internal enum dispatching message mutations through a shared code path.
#[derive(Clone, Copy)]
enum MessageMutation<'a> {
    SetKeywords(&'a SetKeywordsCommand),
    ReplaceMailboxes(&'a ReplaceMailboxesCommand),
    Destroy,
}

/// Orchestrates domain logic by composing gateway, store, and config ports.
///
/// `MailService` is the primary entry point for all business operations.
/// It owns no I/O or live connection registry -- external interactions flow
/// through explicit trait objects supplied by the application layer.
///
/// @spec docs/L0-api#rust-owns-everything
pub struct MailService {
    config: SharedConfigRepository,
    mailbox_reader: Arc<dyn MailboxReadStore>,
    message_lister: Arc<dyn MessageListStore>,
    conversation_reader: Arc<dyn ConversationReadStore>,
    message_detail_reader: Arc<dyn MessageDetailStore>,
    smart_mailboxes: Arc<dyn SmartMailboxStore>,
    sync_state: Arc<dyn SyncStateStore>,
    message_mailboxes: Arc<dyn MessageMailboxStore>,
    sync_writer: Arc<dyn SyncWriteStore>,
    message_commands: Arc<dyn MessageCommandStore>,
    events: Arc<dyn EventStore>,
    source_projections: Arc<dyn SourceProjectionStore>,
    source_data: Arc<dyn SourceDataStore>,
}

impl MailService {
    /// Create a new service with the given store and config repository.
    pub fn new<T>(store: Arc<T>, config: Arc<dyn ConfigRepository>) -> Self
    where
        T: MailStore + 'static,
    {
        Self {
            config,
            mailbox_reader: store.clone(),
            message_lister: store.clone(),
            conversation_reader: store.clone(),
            message_detail_reader: store.clone(),
            smart_mailboxes: store.clone(),
            sync_state: store.clone(),
            message_mailboxes: store.clone(),
            sync_writer: store.clone(),
            message_commands: store.clone(),
            events: store.clone(),
            source_projections: store.clone(),
            source_data: store,
        }
    }

    // -- Config delegates --

    /// Read global application settings.
    ///
    /// @spec docs/L1-api#settings
    pub fn get_app_settings(&self) -> Result<AppSettings, ServiceError> {
        self.config.get_app_settings().map_err(Into::into)
    }

    /// Persist updated global application settings.
    ///
    /// @spec docs/L1-api#settings
    pub fn put_app_settings(&self, settings: &AppSettings) -> Result<(), ServiceError> {
        self.config.put_app_settings(settings).map_err(Into::into)
    }

    /// List all account configurations.
    ///
    /// @spec docs/L1-api#accounts
    pub fn list_sources(&self) -> Result<Vec<AccountSettings>, ServiceError> {
        self.config.list_sources().map_err(Into::into)
    }

    /// Look up a single account configuration by ID.
    pub fn get_source(&self, id: &AccountId) -> Result<Option<AccountSettings>, ServiceError> {
        self.config.get_source(id).map_err(Into::into)
    }

    /// Create or update an account, syncing the source projection in the store.
    ///
    /// @spec docs/L1-api#account-crud-lifecycle
    pub fn save_source(&self, source: &AccountSettings) -> Result<(), ServiceError> {
        self.config.save_source(source)?;
        self.source_projections
            .upsert_source_projection(&source.id, &source.name)?;
        Ok(())
    }

    /// Delete an account: remove config, projection, and all synced data.
    ///
    /// @spec docs/L1-api#account-crud-lifecycle
    pub fn delete_source(&self, id: &AccountId) -> Result<(), ServiceError> {
        let mut settings = self.config.get_app_settings()?;
        if settings.default_account_id.as_ref() == Some(id) {
            settings.default_account_id = None;
            self.config.put_app_settings(&settings)?;
        }
        self.config.delete_source(id)?;
        self.source_projections.delete_source_projection(id)?;
        self.source_data.delete_source_data(id)?;
        Ok(())
    }

    /// List smart mailbox configurations (without live counts).
    pub fn list_smart_mailboxes_config(&self) -> Result<Vec<SmartMailbox>, ServiceError> {
        self.config.list_smart_mailboxes().map_err(Into::into)
    }

    /// Fetch a single smart mailbox configuration, or 404.
    pub fn get_smart_mailbox(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<SmartMailbox, ServiceError> {
        self.config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())
    }

    /// Create or update a smart mailbox configuration.
    ///
    /// @spec docs/L1-api#smart-mailbox-crud
    pub fn save_smart_mailbox(&self, smart_mailbox: &SmartMailbox) -> Result<(), ServiceError> {
        self.config
            .save_smart_mailbox(smart_mailbox)
            .map_err(Into::into)
    }

    /// Delete a smart mailbox configuration.
    pub fn delete_smart_mailbox(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<(), ServiceError> {
        self.config
            .delete_smart_mailbox(smart_mailbox_id)
            .map_err(Into::into)
    }

    /// Restore all default smart mailboxes, preserving user-created ones.
    ///
    /// @spec docs/L1-accounts#smart-mailbox-defaults
    pub fn reset_default_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ServiceError> {
        self.config
            .reset_default_smart_mailboxes()
            .map_err(Into::into)
    }

    /// Re-read config from disk, diff it, and sync source projections.
    ///
    /// @spec docs/L1-accounts#configdiff
    pub fn reload_config(&self) -> Result<ConfigDiff, ServiceError> {
        let diff = self.config.reload()?;
        for source_id in &diff.removed_sources {
            self.source_projections
                .delete_source_projection(source_id)?;
            self.source_data.delete_source_data(source_id)?;
        }
        // Sync all source projections after reload
        self.sync_source_projections()?;
        Ok(diff)
    }

    /// Upsert source projection rows for all configured accounts.
    pub fn sync_source_projections(&self) -> Result<(), ServiceError> {
        let sources = self.config.list_sources()?;
        for source in &sources {
            self.source_projections
                .upsert_source_projection(&source.id, &source.name)?;
        }
        Ok(())
    }

    // -- Composed queries (config + store) --

    /// List smart mailboxes with live unread/total counts from the store.
    ///
    /// @spec docs/L1-api#smart-mailboxes
    pub fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailboxSummary>, ServiceError> {
        let mailboxes = self.config.list_smart_mailboxes()?;
        let mut summaries = Vec::with_capacity(mailboxes.len());
        for mailbox in mailboxes {
            let (unread, total) = self
                .smart_mailboxes
                .query_smart_mailbox_counts(&mailbox.rule)?;
            summaries.push(SmartMailboxSummary {
                id: mailbox.id,
                name: mailbox.name,
                position: mailbox.position,
                kind: mailbox.kind,
                default_key: mailbox.default_key,
                parent_id: mailbox.parent_id,
                unread_messages: unread,
                total_messages: total,
                created_at: mailbox.created_at,
                updated_at: mailbox.updated_at,
            });
        }
        Ok(summaries)
    }

    /// List messages matching a smart mailbox's rule.
    ///
    /// @spec docs/L1-api#smart-mailboxes
    pub fn list_smart_mailbox_messages(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        let mailbox = self
            .config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())?;
        self.smart_mailboxes
            .query_messages_by_rule(&mailbox.rule)
            .map_err(Into::into)
    }

    /// Paginated messages matching a smart mailbox's rule.
    ///
    /// @spec docs/L1-api#smart-mailboxes
    pub fn list_smart_mailbox_message_page(
        &self,
        smart_mailbox_id: &SmartMailboxId,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, ServiceError> {
        let mailbox = self
            .config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())?;
        self.smart_mailboxes
            .query_message_page_by_rule(&mailbox.rule, limit, cursor, sort_field, sort_direction)
            .map_err(Into::into)
    }

    /// List messages matching an explicit smart mailbox rule.
    ///
    /// @spec docs/L1-search#execution-pipeline
    pub fn query_messages_by_rule(
        &self,
        rule: &SmartMailboxRule,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        self.smart_mailboxes
            .query_messages_by_rule(rule)
            .map_err(Into::into)
    }

    /// Paginated messages matching an explicit smart mailbox rule.
    ///
    /// @spec docs/L1-search#execution-pipeline
    pub fn query_message_page_by_rule(
        &self,
        rule: &SmartMailboxRule,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, ServiceError> {
        self.smart_mailboxes
            .query_message_page_by_rule(rule, limit, cursor, sort_field, sort_direction)
            .map_err(Into::into)
    }

    /// Paginated conversations matching a smart mailbox's rule.
    ///
    /// @spec docs/L1-api#smart-mailboxes
    pub fn list_smart_mailbox_conversations(
        &self,
        smart_mailbox_id: &SmartMailboxId,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, ServiceError> {
        let mailbox = self
            .config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())?;
        self.smart_mailboxes
            .query_conversations_by_rule(&mailbox.rule, limit, cursor, sort_field, sort_direction)
            .map_err(Into::into)
    }

    /// Query conversations matching an arbitrary rule (used by search).
    pub fn query_conversations_by_rule(
        &self,
        rule: &SmartMailboxRule,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, ServiceError> {
        self.smart_mailboxes
            .query_conversations_by_rule(rule, limit, cursor, sort_field, sort_direction)
            .map_err(Into::into)
    }

    /// Build the full sidebar: smart mailboxes with counts + per-source mailboxes.
    ///
    /// @spec docs/L1-api#navigation
    pub fn get_sidebar(&self) -> Result<SidebarResponse, ServiceError> {
        let smart_mailboxes = self.config.list_smart_mailboxes()?;
        let sources = self.config.list_sources()?;

        let sidebar_smart_mailboxes: Vec<SidebarSmartMailbox> = smart_mailboxes
            .into_iter()
            .map(|mailbox| -> Result<SidebarSmartMailbox, ServiceError> {
                let (unread, total) = self
                    .smart_mailboxes
                    .query_smart_mailbox_counts(&mailbox.rule)?;
                Ok(SidebarSmartMailbox {
                    id: mailbox.id,
                    name: mailbox.name,
                    unread_messages: unread,
                    total_messages: total,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let sidebar_sources: Vec<SidebarSource> = sources
            .into_iter()
            .filter(|source| source.enabled)
            .map(|source| -> Result<SidebarSource, ServiceError> {
                let mailboxes = self.mailbox_reader.list_mailboxes(&source.id)?;
                Ok(SidebarSource {
                    id: source.id,
                    name: source.name,
                    mailboxes,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(SidebarResponse {
            smart_mailboxes: sidebar_smart_mailboxes,
            sources: sidebar_sources,
        })
    }

    // -- Store delegates (runtime data) --

    /// List all mailboxes for an account.
    pub fn list_mailboxes(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<MailboxSummary>, ServiceError> {
        self.mailbox_reader
            .list_mailboxes(account_id)
            .map_err(Into::into)
    }

    /// Update server-side mailbox metadata and refresh the local mailbox projection.
    ///
    /// @spec docs/L1-api#conversations-and-messages
    /// @spec docs/L1-jmap#methods-used
    pub async fn set_mailbox_role(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
        role: Option<&str>,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let expected_state = self
            .sync_state
            .get_cursor(account_id, SyncObject::Mailbox)?;
        let clear_role_from = match role {
            Some(role) => self
                .mailbox_reader
                .list_mailboxes(account_id)?
                .into_iter()
                .find(|mailbox| mailbox.id != *mailbox_id && mailbox.role.as_deref() == Some(role))
                .map(|mailbox| mailbox.id),
            None => None,
        };
        gateway
            .set_mailbox_role(
                account_id,
                mailbox_id,
                expected_state.as_ref().map(|cursor| cursor.state.as_str()),
                role,
                clear_role_from.as_ref(),
            )
            .await?;
        self.sync_account(account_id, SyncTrigger::Manual, gateway)
            .await
    }

    /// List messages, optionally filtered by mailbox.
    pub fn list_messages(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        self.message_lister
            .list_messages(account_id, mailbox_id)
            .map_err(Into::into)
    }

    /// Paginated message list with seek-based cursors.
    ///
    /// @spec docs/L1-api#conversations-and-messages
    pub fn list_message_page(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, ServiceError> {
        self.message_lister
            .list_message_page(
                account_id,
                mailbox_id,
                limit,
                cursor,
                sort_field,
                sort_direction,
            )
            .map_err(Into::into)
    }

    /// Paginated conversation list with seek-based cursors.
    ///
    /// @spec docs/L1-api#conversations-and-messages
    pub fn list_conversations(
        &self,
        account_id: Option<&AccountId>,
        mailbox_id: Option<&MailboxId>,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, ServiceError> {
        self.conversation_reader
            .list_conversations(
                account_id,
                mailbox_id,
                limit,
                cursor,
                sort_field,
                sort_direction,
            )
            .map_err(Into::into)
    }

    /// Fetch a single conversation with all its messages, or 404.
    pub fn get_conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ConversationView, ServiceError> {
        self.conversation_reader
            .get_conversation(conversation_id)?
            .not_found("conversation", conversation_id.as_str())
    }

    /// Fetch all messages in a thread, or 404.
    pub fn get_thread(
        &self,
        account_id: &AccountId,
        thread_id: &ThreadId,
    ) -> Result<ThreadView, ServiceError> {
        self.message_detail_reader
            .get_thread(account_id, thread_id)?
            .not_found("thread", thread_id.as_str())
    }

    /// Fetch message detail, lazily fetching body from the gateway if needed.
    ///
    /// @spec docs/L1-sync#sync-loop
    pub async fn get_message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        gateway: Option<&dyn MailGateway>,
    ) -> Result<CommandResult, ServiceError> {
        let detail = self
            .message_detail_reader
            .get_message_detail(account_id, message_id)?
            .not_found("message", message_id.as_str())?;

        let body_loaded = detail.body_html.is_some() || detail.body_text.is_some();
        let attachments_loaded = !detail.summary.has_attachment || !detail.attachments.is_empty();
        if body_loaded && attachments_loaded {
            return Ok(CommandResult {
                detail: Some(detail),
                events: Vec::new(),
            });
        }

        let Some(gateway) = gateway else {
            return Ok(CommandResult {
                detail: Some(detail),
                events: Vec::new(),
            });
        };

        let fetched = gateway.fetch_message_body(account_id, message_id).await?;
        self.sync_writer
            .apply_message_body(account_id, message_id, &fetched)
            .map_err(Into::into)
    }

    /// Download a blob for a specific account via the registered gateway.
    pub async fn download_blob(
        &self,
        account_id: &AccountId,
        blob_id: &crate::BlobId,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<u8>, ServiceError> {
        gateway
            .download_blob(account_id, blob_id)
            .await
            .map_err(Into::into)
    }

    /// Run a full sync cycle: load cursors, fetch delta, apply batch, emit events.
    ///
    /// @spec docs/L1-sync#sync-loop
    pub async fn sync_account(
        &self,
        account_id: &AccountId,
        trigger: SyncTrigger,
        gateway: &dyn MailGateway,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let cursors = self.sync_state.get_sync_cursors(account_id)?;
        let batch = gateway.sync(account_id, &cursors).await?;
        let mut events = self.sync_writer.apply_sync_batch(account_id, &batch)?;
        let sync_event = self.events.append_event(
            account_id,
            EVENT_TOPIC_SYNC_COMPLETED,
            None,
            None,
            json!({
                "mailboxCount": batch.mailboxes.len(),
                "messageCount": batch.messages.len(),
                "deletedMessageCount": batch.deleted_message_ids.len(),
                "trigger": trigger.as_str(),
            }),
        )?;
        events.push(sync_event);
        Ok(events)
    }

    /// Append a `sync.failed` event to the event log.
    ///
    /// @spec docs/L1-sync#error-handling
    pub fn record_sync_failure(
        &self,
        account_id: &AccountId,
        code: &str,
        message: &str,
        trigger: SyncTrigger,
        stage: &str,
    ) -> Result<DomainEvent, ServiceError> {
        self.events
            .append_event(
                account_id,
                EVENT_TOPIC_SYNC_FAILED,
                None,
                None,
                json!({
                    "code": code,
                    "message": message,
                    "trigger": trigger.as_str(),
                    "stage": stage,
                }),
            )
            .map_err(Into::into)
    }

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

    /// Query the event log with optional filters.
    ///
    /// @spec docs/L1-api#sse-event-stream
    pub fn list_events(
        &self,
        filter: &crate::EventFilter,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        self.events.list_events(filter).map_err(Into::into)
    }

    /// Fetch the primary sender identity from the gateway.
    ///
    /// @spec docs/L1-jmap#methods-used
    pub async fn fetch_identity(
        &self,
        account_id: &AccountId,
        gateway: &dyn MailGateway,
    ) -> Result<Identity, ServiceError> {
        gateway.fetch_identity(account_id).await.map_err(Into::into)
    }

    /// Fetch reply/forward metadata for composing a response.
    pub async fn fetch_reply_context(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        gateway: &dyn MailGateway,
    ) -> Result<crate::ReplyContext, ServiceError> {
        gateway
            .fetch_reply_context(account_id, message_id)
            .await
            .map_err(Into::into)
    }

    /// Send an email via the gateway.
    ///
    /// @spec docs/L1-jmap#methods-used
    pub async fn send_message(
        &self,
        account_id: &AccountId,
        request: &SendMessageRequest,
        gateway: &dyn MailGateway,
    ) -> Result<(), ServiceError> {
        gateway
            .send_message(account_id, request)
            .await
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use super::*;
    use crate::{
        ConfigError, ConfigSnapshot, ConversationReadStore, DomainEvent, EventFilter, EventStore,
        FetchedBody, GatewayError, MailboxReadStore, MessageCommandStore, MessageDetail,
        MessageDetailStore, MessageListStore, MessageMailboxStore, MutationOutcome, PushTransport,
        SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
        SmartMailboxKind, SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode,
        SmartMailboxStore, SmartMailboxValue, SourceDataStore, SourceProjectionStore, StoreError,
        SyncBatch, SyncCursor, SyncStateStore, SyncWriteStore,
    };

    struct TestConfig {
        smart_mailboxes: Vec<SmartMailbox>,
        sources: Vec<AccountSettings>,
        reload_diff: ConfigDiff,
        app_settings: Mutex<AppSettings>,
        deleted_sources: Mutex<Vec<AccountId>>,
    }

    impl Default for TestConfig {
        fn default() -> Self {
            Self {
                smart_mailboxes: Vec::new(),
                sources: Vec::new(),
                reload_diff: ConfigDiff {
                    added_sources: Vec::new(),
                    changed_sources: Vec::new(),
                    removed_sources: Vec::new(),
                },
                app_settings: Mutex::new(AppSettings::default()),
                deleted_sources: Mutex::new(Vec::new()),
            }
        }
    }

    impl ConfigRepository for TestConfig {
        fn load_snapshot(&self) -> Result<ConfigSnapshot, ConfigError> {
            Ok(ConfigSnapshot {
                app_settings: self.get_app_settings()?,
                sources: self.sources.clone(),
                smart_mailboxes: self.smart_mailboxes.clone(),
            })
        }

        fn reload(&self) -> Result<ConfigDiff, ConfigError> {
            Ok(self.reload_diff.clone())
        }

        fn get_app_settings(&self) -> Result<AppSettings, ConfigError> {
            Ok(self
                .app_settings
                .lock()
                .expect("app settings lock poisoned")
                .clone())
        }

        fn put_app_settings(&self, settings: &AppSettings) -> Result<(), ConfigError> {
            *self
                .app_settings
                .lock()
                .expect("app settings lock poisoned") = settings.clone();
            Ok(())
        }

        fn list_sources(&self) -> Result<Vec<AccountSettings>, ConfigError> {
            Ok(self.sources.clone())
        }

        fn get_source(&self, id: &AccountId) -> Result<Option<AccountSettings>, ConfigError> {
            Ok(self.sources.iter().find(|source| &source.id == id).cloned())
        }

        fn save_source(&self, _source: &AccountSettings) -> Result<(), ConfigError> {
            Ok(())
        }

        fn delete_source(&self, id: &AccountId) -> Result<(), ConfigError> {
            self.deleted_sources
                .lock()
                .expect("deleted sources lock poisoned")
                .push(id.clone());
            Ok(())
        }

        fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError> {
            Ok(self.smart_mailboxes.clone())
        }

        fn get_smart_mailbox(
            &self,
            id: &SmartMailboxId,
        ) -> Result<Option<SmartMailbox>, ConfigError> {
            Ok(self
                .smart_mailboxes
                .iter()
                .find(|mailbox| &mailbox.id == id)
                .cloned())
        }

        fn save_smart_mailbox(&self, _mailbox: &SmartMailbox) -> Result<(), ConfigError> {
            Ok(())
        }

        fn delete_smart_mailbox(&self, _id: &SmartMailboxId) -> Result<(), ConfigError> {
            Ok(())
        }

        fn reset_default_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError> {
            Ok(self.smart_mailboxes.clone())
        }
    }

    struct TestStore {
        smart_mailbox_counts_error: Option<String>,
        list_mailboxes_error: Option<String>,
        projection_calls: Mutex<Vec<String>>,
        projection_deletes: Mutex<Vec<String>>,
        source_data_deletes: Mutex<Vec<String>>,
        mutation_state: Mutex<MutationStoreState>,
    }

    impl Default for TestStore {
        fn default() -> Self {
            Self {
                smart_mailbox_counts_error: None,
                list_mailboxes_error: None,
                projection_calls: Mutex::new(Vec::new()),
                projection_deletes: Mutex::new(Vec::new()),
                source_data_deletes: Mutex::new(Vec::new()),
                mutation_state: Mutex::new(MutationStoreState::default()),
            }
        }
    }

    #[derive(Default)]
    struct MutationStoreState {
        cursor: Option<SyncCursor>,
        mailbox_ids: Vec<MailboxId>,
    }

    impl TestStore {
        fn with_message_state(cursor_state: &str, mailbox_ids: &[&str]) -> Self {
            Self {
                mutation_state: Mutex::new(MutationStoreState {
                    cursor: Some(SyncCursor {
                        object_type: SyncObject::Message,
                        state: cursor_state.to_string(),
                        updated_at: crate::RFC3339_EPOCH.to_string(),
                    }),
                    mailbox_ids: mailbox_ids.iter().map(|id| MailboxId::from(*id)).collect(),
                }),
                ..Default::default()
            }
        }
    }

    impl MailboxReadStore for TestStore {
        fn list_mailboxes(
            &self,
            _account_id: &AccountId,
        ) -> Result<Vec<MailboxSummary>, StoreError> {
            self.list_mailboxes_error
                .as_ref()
                .map_or(Ok(Vec::new()), |error| {
                    Err(StoreError::Failure(error.clone()))
                })
        }
    }

    impl MessageListStore for TestStore {
        fn list_messages(
            &self,
            _account_id: &AccountId,
            _mailbox_id: Option<&MailboxId>,
        ) -> Result<Vec<MessageSummary>, StoreError> {
            Ok(Vec::new())
        }

        fn list_message_page(
            &self,
            _account_id: &AccountId,
            _mailbox_id: Option<&MailboxId>,
            _limit: usize,
            _cursor: Option<&MessageCursor>,
            _sort_field: MessageSortField,
            _sort_direction: SortDirection,
        ) -> Result<MessagePage, StoreError> {
            Ok(MessagePage {
                items: Vec::new(),
                next_cursor: None,
            })
        }
    }

    impl SmartMailboxStore for TestStore {
        fn query_messages_by_rule(
            &self,
            _rule: &SmartMailboxRule,
        ) -> Result<Vec<MessageSummary>, StoreError> {
            Ok(Vec::new())
        }

        fn query_message_page_by_rule(
            &self,
            _rule: &SmartMailboxRule,
            _limit: usize,
            _cursor: Option<&MessageCursor>,
            _sort_field: MessageSortField,
            _sort_direction: SortDirection,
        ) -> Result<MessagePage, StoreError> {
            Ok(MessagePage {
                items: Vec::new(),
                next_cursor: None,
            })
        }

        fn query_conversations_by_rule(
            &self,
            _rule: &SmartMailboxRule,
            _limit: usize,
            _cursor: Option<&ConversationCursor>,
            _sort_field: ConversationSortField,
            _sort_direction: SortDirection,
        ) -> Result<ConversationPage, StoreError> {
            Ok(ConversationPage {
                items: Vec::new(),
                next_cursor: None,
            })
        }

        fn query_smart_mailbox_counts(
            &self,
            _rule: &SmartMailboxRule,
        ) -> Result<(i64, i64), StoreError> {
            self.smart_mailbox_counts_error
                .as_ref()
                .map_or(Ok((1, 2)), |error| Err(StoreError::Failure(error.clone())))
        }
    }

    impl ConversationReadStore for TestStore {
        fn list_conversations(
            &self,
            _account_id: Option<&AccountId>,
            _mailbox_id: Option<&MailboxId>,
            _limit: usize,
            _cursor: Option<&ConversationCursor>,
            _sort_field: ConversationSortField,
            _sort_direction: SortDirection,
        ) -> Result<ConversationPage, StoreError> {
            Ok(ConversationPage {
                items: Vec::new(),
                next_cursor: None,
            })
        }

        fn get_conversation(
            &self,
            _conversation_id: &ConversationId,
        ) -> Result<Option<ConversationView>, StoreError> {
            Ok(None)
        }
    }

    impl MessageDetailStore for TestStore {
        fn get_message_detail(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
        ) -> Result<Option<MessageDetail>, StoreError> {
            Ok(None)
        }

        fn get_thread(
            &self,
            _account_id: &AccountId,
            _thread_id: &ThreadId,
        ) -> Result<Option<ThreadView>, StoreError> {
            Ok(None)
        }
    }

    impl SyncStateStore for TestStore {
        fn get_sync_cursors(&self, _account_id: &AccountId) -> Result<Vec<SyncCursor>, StoreError> {
            Ok(Vec::new())
        }

        fn get_cursor(
            &self,
            _account_id: &AccountId,
            object_type: SyncObject,
        ) -> Result<Option<SyncCursor>, StoreError> {
            if object_type == SyncObject::Message {
                return Ok(self
                    .mutation_state
                    .lock()
                    .expect("mutation state lock poisoned")
                    .cursor
                    .clone());
            }
            Ok(None)
        }
    }

    impl MessageMailboxStore for TestStore {
        fn get_message_mailboxes(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
        ) -> Result<Vec<MailboxId>, StoreError> {
            Ok(self
                .mutation_state
                .lock()
                .expect("mutation state lock poisoned")
                .mailbox_ids
                .clone())
        }
    }

    impl SyncWriteStore for TestStore {
        fn apply_sync_batch(
            &self,
            _account_id: &AccountId,
            _batch: &SyncBatch,
        ) -> Result<Vec<DomainEvent>, StoreError> {
            Ok(Vec::new())
        }

        fn apply_message_body(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            _body: &FetchedBody,
        ) -> Result<CommandResult, StoreError> {
            Err(StoreError::Failure("unused".to_string()))
        }
    }

    impl MessageCommandStore for TestStore {
        fn set_keywords(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            cursor: Option<&SyncCursor>,
            _command: &SetKeywordsCommand,
        ) -> Result<CommandResult, StoreError> {
            if let Some(cursor) = cursor {
                self.mutation_state
                    .lock()
                    .expect("mutation state lock poisoned")
                    .cursor = Some(cursor.clone());
            }
            Ok(CommandResult {
                detail: None,
                events: Vec::new(),
            })
        }

        fn replace_mailboxes(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            cursor: Option<&SyncCursor>,
            command: &ReplaceMailboxesCommand,
        ) -> Result<CommandResult, StoreError> {
            let mut state = self
                .mutation_state
                .lock()
                .expect("mutation state lock poisoned");
            state.mailbox_ids = command.mailbox_ids.clone();
            if let Some(cursor) = cursor {
                state.cursor = Some(cursor.clone());
            }
            Ok(CommandResult {
                detail: None,
                events: Vec::new(),
            })
        }

        fn destroy_message(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            cursor: Option<&SyncCursor>,
        ) -> Result<CommandResult, StoreError> {
            let mut state = self
                .mutation_state
                .lock()
                .expect("mutation state lock poisoned");
            state.mailbox_ids.clear();
            if let Some(cursor) = cursor {
                state.cursor = Some(cursor.clone());
            }
            Ok(CommandResult {
                detail: None,
                events: Vec::new(),
            })
        }
    }

    impl EventStore for TestStore {
        fn list_events(&self, _filter: &EventFilter) -> Result<Vec<DomainEvent>, StoreError> {
            Ok(Vec::new())
        }

        fn append_event(
            &self,
            _account_id: &AccountId,
            _topic: &str,
            _mailbox_id: Option<&MailboxId>,
            _message_id: Option<&MessageId>,
            _payload: serde_json::Value,
        ) -> Result<DomainEvent, StoreError> {
            Err(StoreError::Failure("unused".to_string()))
        }
    }

    impl SourceProjectionStore for TestStore {
        fn upsert_source_projection(
            &self,
            source_id: &AccountId,
            _name: &str,
        ) -> Result<(), StoreError> {
            self.projection_calls
                .lock()
                .expect("projection lock poisoned")
                .push(source_id.to_string());
            Ok(())
        }

        fn delete_source_projection(&self, source_id: &AccountId) -> Result<(), StoreError> {
            self.projection_deletes
                .lock()
                .expect("projection deletes lock poisoned")
                .push(source_id.to_string());
            Ok(())
        }
    }

    impl SourceDataStore for TestStore {
        fn delete_source_data(&self, account_id: &AccountId) -> Result<(), StoreError> {
            self.source_data_deletes
                .lock()
                .expect("source data deletes lock poisoned")
                .push(account_id.to_string());
            Ok(())
        }
    }

    fn sample_smart_mailbox() -> SmartMailbox {
        SmartMailbox {
            id: SmartMailboxId::from("default-inbox"),
            name: "Inbox".to_string(),
            position: 0,
            kind: SmartMailboxKind::Default,
            default_key: Some("inbox".to_string()),
            parent_id: None,
            rule: SmartMailboxRule {
                root: SmartMailboxGroup {
                    operator: SmartMailboxGroupOperator::All,
                    negated: false,
                    nodes: vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                        field: SmartMailboxField::MailboxRole,
                        operator: SmartMailboxOperator::Equals,
                        negated: false,
                        value: SmartMailboxValue::String("inbox".to_string()),
                    })],
                },
            },
            created_at: crate::RFC3339_EPOCH.to_string(),
            updated_at: crate::RFC3339_EPOCH.to_string(),
        }
    }

    fn sample_source() -> AccountSettings {
        AccountSettings {
            id: AccountId::from("primary"),
            name: "Primary".to_string(),
            full_name: None,
            email_patterns: Vec::new(),
            driver: crate::AccountDriver::Mock,
            enabled: true,
            appearance: None,
            transport: Default::default(),
            created_at: crate::RFC3339_EPOCH.to_string(),
            updated_at: crate::RFC3339_EPOCH.to_string(),
        }
    }

    struct MutationGateway {
        revision: Mutex<u64>,
    }

    impl MutationGateway {
        fn with_revision(revision: u64) -> Self {
            Self {
                revision: Mutex::new(revision),
            }
        }

        fn apply(&self, expected_state: Option<&str>) -> Result<MutationOutcome, GatewayError> {
            let mut revision = self.revision.lock().expect("revision lock poisoned");
            if let Some(expected_state) = expected_state {
                let current = format!("message-{}", *revision);
                if expected_state != current {
                    return Err(GatewayError::StateMismatch);
                }
            }
            *revision += 1;
            Ok(MutationOutcome {
                cursor: Some(SyncCursor {
                    object_type: SyncObject::Message,
                    state: format!("message-{}", *revision),
                    updated_at: crate::RFC3339_EPOCH.to_string(),
                }),
            })
        }
    }

    #[async_trait]
    impl MailGateway for MutationGateway {
        async fn sync(
            &self,
            _account_id: &AccountId,
            _cursors: &[SyncCursor],
        ) -> Result<SyncBatch, GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn fetch_message_body(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
        ) -> Result<FetchedBody, GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn download_blob(
            &self,
            _account_id: &AccountId,
            _blob_id: &crate::BlobId,
        ) -> Result<Vec<u8>, GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn set_keywords(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            expected_state: Option<&str>,
            _command: &SetKeywordsCommand,
        ) -> Result<MutationOutcome, GatewayError> {
            self.apply(expected_state)
        }

        async fn replace_mailboxes(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            expected_state: Option<&str>,
            _mailbox_ids: &[MailboxId],
        ) -> Result<MutationOutcome, GatewayError> {
            self.apply(expected_state)
        }

        async fn destroy_message(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            expected_state: Option<&str>,
        ) -> Result<MutationOutcome, GatewayError> {
            self.apply(expected_state)
        }

        async fn set_mailbox_role(
            &self,
            _account_id: &AccountId,
            _mailbox_id: &MailboxId,
            _expected_state: Option<&str>,
            _role: Option<&str>,
            _clear_role_from: Option<&MailboxId>,
        ) -> Result<MutationOutcome, GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn fetch_identity(&self, _account_id: &AccountId) -> Result<Identity, GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn fetch_reply_context(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
        ) -> Result<crate::ReplyContext, GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn send_message(
            &self,
            _account_id: &AccountId,
            _request: &SendMessageRequest,
        ) -> Result<(), GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        fn push_transports(&self) -> Vec<Box<dyn PushTransport>> {
            vec![]
        }
    }

    #[test]
    fn list_smart_mailboxes_propagates_store_count_errors() {
        let store = Arc::new(TestStore {
            smart_mailbox_counts_error: Some("counts failed".to_string()),
            ..Default::default()
        });
        let config = Arc::new(TestConfig {
            smart_mailboxes: vec![sample_smart_mailbox()],
            sources: Vec::new(),
            ..Default::default()
        });
        let service = MailService::new(store, config);

        let error = service
            .list_smart_mailboxes()
            .expect_err("count failures should not be swallowed");

        assert_eq!(error.code(), "storage_failure");
    }

    #[test]
    fn get_sidebar_propagates_mailbox_listing_errors() {
        let store = Arc::new(TestStore {
            list_mailboxes_error: Some("mailboxes failed".to_string()),
            ..Default::default()
        });
        let config = Arc::new(TestConfig {
            smart_mailboxes: vec![sample_smart_mailbox()],
            sources: vec![sample_source()],
            ..Default::default()
        });
        let service = MailService::new(store, config);

        let error = service
            .get_sidebar()
            .expect_err("mailbox failures should not be swallowed");

        assert_eq!(error.code(), "storage_failure");
    }

    #[tokio::test]
    async fn consecutive_keyword_mutations_advance_message_cursor() {
        let account = AccountId::from("primary");
        let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
        let config = Arc::new(TestConfig::default());
        let service = MailService::new(store.clone(), config);
        let gateway = MutationGateway::with_revision(1);

        service
            .set_keywords(
                &account,
                &MessageId::from("message-1"),
                &SetKeywordsCommand {
                    add: vec!["$flagged".to_string()],
                    remove: Vec::new(),
                },
                &gateway,
            )
            .await
            .expect("flagging should succeed");
        assert_eq!(
            store
                .get_cursor(&account, SyncObject::Message)
                .expect("cursor lookup should succeed")
                .expect("cursor should exist")
                .state,
            "message-2"
        );

        service
            .set_keywords(
                &account,
                &MessageId::from("message-1"),
                &SetKeywordsCommand {
                    add: Vec::new(),
                    remove: vec!["$flagged".to_string()],
                },
                &gateway,
            )
            .await
            .expect("unflagging should succeed");
        assert_eq!(
            store
                .get_cursor(&account, SyncObject::Message)
                .expect("cursor lookup should succeed")
                .expect("cursor should exist")
                .state,
            "message-3"
        );
    }

    #[tokio::test]
    async fn mixed_message_mutations_reuse_advanced_cursor() {
        let account = AccountId::from("primary");
        let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
        let config = Arc::new(TestConfig::default());
        let service = MailService::new(store.clone(), config);
        let gateway = MutationGateway::with_revision(1);

        service
            .set_keywords(
                &account,
                &MessageId::from("message-1"),
                &SetKeywordsCommand {
                    add: vec!["$flagged".to_string()],
                    remove: Vec::new(),
                },
                &gateway,
            )
            .await
            .expect("first mutation should succeed");
        service
            .replace_mailboxes(
                &account,
                &MessageId::from("message-1"),
                &ReplaceMailboxesCommand {
                    mailbox_ids: vec![MailboxId::from("archive")],
                },
                &gateway,
            )
            .await
            .expect("second mutation should succeed");

        assert_eq!(
            store
                .get_cursor(&account, SyncObject::Message)
                .expect("cursor lookup should succeed")
                .expect("cursor should exist")
                .state,
            "message-3"
        );
        assert_eq!(
            store
                .get_message_mailboxes(&account, &MessageId::from("message-1"))
                .expect("mailbox lookup should succeed"),
            vec![MailboxId::from("archive")]
        );
    }

    #[tokio::test]
    async fn genuine_state_mismatch_is_not_retried() {
        let account = AccountId::from("primary");
        let store = Arc::new(TestStore::with_message_state("message-1", &["inbox"]));
        let config = Arc::new(TestConfig::default());
        let service = MailService::new(store.clone(), config);
        let gateway = MutationGateway::with_revision(2);

        let error = service
            .set_keywords(
                &account,
                &MessageId::from("message-1"),
                &SetKeywordsCommand {
                    add: vec!["$flagged".to_string()],
                    remove: Vec::new(),
                },
                &gateway,
            )
            .await
            .expect_err("mismatch should be returned to the caller");

        assert_eq!(error.code(), "state_mismatch");
        assert_eq!(
            store
                .get_cursor(&account, SyncObject::Message)
                .expect("cursor lookup should succeed")
                .expect("cursor should exist")
                .state,
            "message-1"
        );
    }

    #[test]
    fn delete_source_clears_default_account_before_removing_it() {
        let account = sample_source();
        let config = Arc::new(TestConfig {
            sources: vec![account.clone()],
            app_settings: Mutex::new(AppSettings {
                default_account_id: Some(account.id.clone()),
            }),
            ..Default::default()
        });
        let store = Arc::new(TestStore::default());
        let service = MailService::new(store.clone(), config.clone());

        service
            .delete_source(&account.id)
            .expect("deleting the account should succeed");

        assert_eq!(
            config
                .get_app_settings()
                .expect("settings lookup should succeed")
                .default_account_id,
            None
        );
        assert_eq!(
            config
                .deleted_sources
                .lock()
                .expect("deleted sources lock poisoned")
                .as_slice(),
            std::slice::from_ref(&account.id)
        );
        assert_eq!(
            store
                .projection_deletes
                .lock()
                .expect("projection deletes lock poisoned")
                .as_slice(),
            &[account.id.to_string()]
        );
        assert_eq!(
            store
                .source_data_deletes
                .lock()
                .expect("source data deletes lock poisoned")
                .as_slice(),
            &[account.id.to_string()]
        );
    }

    #[test]
    fn reload_config_cleans_up_removed_sources_before_resyncing_projections() {
        let removed = AccountId::from("removed");
        let remaining = sample_source();
        let config = Arc::new(TestConfig {
            sources: vec![remaining.clone()],
            reload_diff: ConfigDiff {
                added_sources: Vec::new(),
                changed_sources: Vec::new(),
                removed_sources: vec![removed.clone()],
            },
            ..Default::default()
        });
        let store = Arc::new(TestStore::default());
        let service = MailService::new(store.clone(), config);

        let diff = service
            .reload_config()
            .expect("reloading config should succeed");

        assert_eq!(diff.removed_sources, vec![removed.clone()]);
        assert_eq!(
            store
                .projection_deletes
                .lock()
                .expect("projection deletes lock poisoned")
                .as_slice(),
            &[removed.to_string()]
        );
        assert_eq!(
            store
                .source_data_deletes
                .lock()
                .expect("source data deletes lock poisoned")
                .as_slice(),
            &[removed.to_string()]
        );
        assert_eq!(
            store
                .projection_calls
                .lock()
                .expect("projection lock poisoned")
                .as_slice(),
            &[remaining.id.to_string()]
        );
    }
}
