use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::json;

use crate::{
    AccountId, AccountSettings, AddToMailboxCommand, AppSettings, CommandResult, ConfigDiff,
    ConfigRepository, ConversationCursor, ConversationId, ConversationPage, ConversationView,
    Identity, MailGateway, MailStore, MailboxId, MailboxSummary, MessageId, MessageSummary,
    MutationOutcome, RemoveFromMailboxCommand, ReplaceMailboxesCommand, SendMessageRequest,
    ServiceError, SetKeywordsCommand, SharedConfigRepository, SharedGateway, SharedStore,
    SidebarResponse, SidebarSmartMailbox, SidebarSource, SmartMailbox, SmartMailboxId,
    SmartMailboxSummary, SyncObject, SyncTrigger, ThreadId, ThreadView, EVENT_TOPIC_SYNC_COMPLETED,
    EVENT_TOPIC_SYNC_FAILED,
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
/// It owns no I/O -- all external interactions flow through injected trait objects.
///
/// @spec spec/L0-api#rust-owns-everything
pub struct MailService {
    store: SharedStore,
    config: SharedConfigRepository,
    gateways: RwLock<HashMap<String, SharedGateway>>,
}

impl MailService {
    /// Create a new service with the given store and config repository.
    pub fn new(store: Arc<dyn MailStore>, config: Arc<dyn ConfigRepository>) -> Self {
        Self {
            store,
            config,
            gateways: RwLock::new(HashMap::new()),
        }
    }

    /// Builder-style: register a gateway for an account (used in tests).
    pub fn with_gateway(mut self, account_id: &AccountId, gateway: Arc<dyn MailGateway>) -> Self {
        self.gateways
            .get_mut()
            .expect("gateway registry lock poisoned")
            .insert(account_id.to_string(), gateway);
        self
    }

    /// Register or replace a gateway for a live account.
    pub fn set_gateway(&self, account_id: &AccountId, gateway: SharedGateway) {
        self.gateways
            .write()
            .expect("gateway registry lock poisoned")
            .insert(account_id.to_string(), gateway);
    }

    /// Unregister a gateway when an account is deleted or disabled.
    pub fn remove_gateway(&self, account_id: &AccountId) {
        self.gateways
            .write()
            .expect("gateway registry lock poisoned")
            .remove(account_id.as_str());
    }

    // -- Config delegates --

    /// Read global application settings.
    ///
    /// @spec spec/L1-api#settings
    pub fn get_app_settings(&self) -> Result<AppSettings, ServiceError> {
        self.config.get_app_settings().map_err(Into::into)
    }

    /// Persist updated global application settings.
    ///
    /// @spec spec/L1-api#settings
    pub fn put_app_settings(&self, settings: &AppSettings) -> Result<(), ServiceError> {
        self.config.put_app_settings(settings).map_err(Into::into)
    }

    /// List all account configurations.
    ///
    /// @spec spec/L1-api#accounts
    pub fn list_sources(&self) -> Result<Vec<AccountSettings>, ServiceError> {
        self.config.list_sources().map_err(Into::into)
    }

    /// Look up a single account configuration by ID.
    pub fn get_source(&self, id: &AccountId) -> Result<Option<AccountSettings>, ServiceError> {
        self.config.get_source(id).map_err(Into::into)
    }

    /// Create or update an account, syncing the source projection in the store.
    ///
    /// @spec spec/L1-api#account-crud-lifecycle
    pub fn save_source(&self, source: &AccountSettings) -> Result<(), ServiceError> {
        self.config.save_source(source)?;
        self.store
            .upsert_source_projection(&source.id, &source.name)?;
        Ok(())
    }

    /// Delete an account: remove config, projection, and all synced data.
    ///
    /// @spec spec/L1-api#account-crud-lifecycle
    pub fn delete_source(&self, id: &AccountId) -> Result<(), ServiceError> {
        self.config.delete_source(id)?;
        self.store.delete_source_projection(id)?;
        self.store.delete_source_data(id)?;
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
    /// @spec spec/L1-api#smart-mailbox-crud
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
    /// @spec spec/L1-accounts#smart-mailbox-defaults
    pub fn reset_default_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ServiceError> {
        self.config
            .reset_default_smart_mailboxes()
            .map_err(Into::into)
    }

    /// Re-read config from disk, diff it, and sync source projections.
    ///
    /// @spec spec/L1-accounts#configdiff
    pub fn reload_config(&self) -> Result<ConfigDiff, ServiceError> {
        let diff = self.config.reload()?;
        // Sync all source projections after reload
        self.sync_source_projections()?;
        Ok(diff)
    }

    /// Upsert source projection rows for all configured accounts.
    pub fn sync_source_projections(&self) -> Result<(), ServiceError> {
        let sources = self.config.list_sources()?;
        for source in &sources {
            self.store
                .upsert_source_projection(&source.id, &source.name)?;
        }
        Ok(())
    }

    // -- Composed queries (config + store) --

    /// List smart mailboxes with live unread/total counts from the store.
    ///
    /// @spec spec/L1-api#smart-mailboxes
    pub fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailboxSummary>, ServiceError> {
        let mailboxes = self.config.list_smart_mailboxes()?;
        let mut summaries = Vec::with_capacity(mailboxes.len());
        for mailbox in mailboxes {
            let (unread, total) = self.store.query_smart_mailbox_counts(&mailbox.rule)?;
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
    /// @spec spec/L1-api#smart-mailboxes
    pub fn list_smart_mailbox_messages(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        let mailbox = self
            .config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())?;
        self.store
            .query_messages_by_rule(&mailbox.rule)
            .map_err(Into::into)
    }

    /// Paginated conversations matching a smart mailbox's rule.
    ///
    /// @spec spec/L1-api#smart-mailboxes
    pub fn list_smart_mailbox_conversations(
        &self,
        smart_mailbox_id: &SmartMailboxId,
        limit: usize,
        cursor: Option<&ConversationCursor>,
    ) -> Result<ConversationPage, ServiceError> {
        let mailbox = self
            .config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())?;
        self.store
            .query_conversations_by_rule(&mailbox.rule, limit, cursor)
            .map_err(Into::into)
    }

    /// Build the full sidebar: smart mailboxes with counts + per-source mailboxes.
    ///
    /// @spec spec/L1-api#navigation
    pub fn get_sidebar(&self) -> Result<SidebarResponse, ServiceError> {
        let smart_mailboxes = self.config.list_smart_mailboxes()?;
        let sources = self.config.list_sources()?;

        let sidebar_smart_mailboxes: Vec<SidebarSmartMailbox> = smart_mailboxes
            .into_iter()
            .map(|mailbox| -> Result<SidebarSmartMailbox, ServiceError> {
                let (unread, total) = self.store.query_smart_mailbox_counts(&mailbox.rule)?;
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
                let mailboxes = self.store.list_mailboxes(&source.id)?;
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
        self.store.list_mailboxes(account_id).map_err(Into::into)
    }

    /// List messages, optionally filtered by mailbox.
    pub fn list_messages(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        self.store
            .list_messages(account_id, mailbox_id)
            .map_err(Into::into)
    }

    /// Paginated conversation list with seek-based cursors.
    ///
    /// @spec spec/L1-api#conversations-and-messages
    pub fn list_conversations(
        &self,
        account_id: Option<&AccountId>,
        mailbox_id: Option<&MailboxId>,
        limit: usize,
        cursor: Option<&ConversationCursor>,
    ) -> Result<ConversationPage, ServiceError> {
        self.store
            .list_conversations(account_id, mailbox_id, limit, cursor)
            .map_err(Into::into)
    }

    /// Fetch a single conversation with all its messages, or 404.
    pub fn get_conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ConversationView, ServiceError> {
        self.store
            .get_conversation(conversation_id)?
            .not_found("conversation", conversation_id.as_str())
    }

    /// Fetch all messages in a thread, or 404.
    pub fn get_thread(
        &self,
        account_id: &AccountId,
        thread_id: &ThreadId,
    ) -> Result<ThreadView, ServiceError> {
        self.store
            .get_thread(account_id, thread_id)?
            .not_found("thread", thread_id.as_str())
    }

    /// Fetch message detail, lazily fetching body from the gateway if needed.
    ///
    /// @spec spec/L1-sync#sync-loop
    pub async fn get_message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<CommandResult, ServiceError> {
        let detail = self
            .store
            .get_message_detail(account_id, message_id)?
            .not_found("message", message_id.as_str())?;

        if detail.body_html.is_some() || detail.body_text.is_some() {
            return Ok(CommandResult {
                detail: Some(detail),
                events: Vec::new(),
            });
        }

        let Some(gateway) = self.gateway(account_id) else {
            return Ok(CommandResult {
                detail: Some(detail),
                events: Vec::new(),
            });
        };

        let fetched = gateway.fetch_message_body(account_id, message_id).await?;
        self.store
            .apply_message_body(account_id, message_id, &fetched)
            .map_err(Into::into)
    }

    /// Run a full sync cycle: load cursors, fetch delta, apply batch, emit events.
    ///
    /// @spec spec/L1-sync#sync-loop
    pub async fn sync_account(
        &self,
        account_id: &AccountId,
        trigger: SyncTrigger,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        let gateway = self.required_gateway(account_id)?;
        let cursors = self.store.get_sync_cursors(account_id)?;
        let batch = gateway.sync(account_id, &cursors).await?;
        let mut events = self.store.apply_sync_batch(account_id, &batch)?;
        let sync_event = self.store.append_event(
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
    /// @spec spec/L1-sync#error-handling
    pub fn record_sync_failure(
        &self,
        account_id: &AccountId,
        code: &str,
        message: &str,
        trigger: SyncTrigger,
        stage: &str,
    ) -> Result<DomainEvent, ServiceError> {
        self.store
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
    /// @spec spec/L1-sync#conflict-model
    async fn apply_message_mutation(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        mutation: MessageMutation<'_>,
    ) -> Result<CommandResult, ServiceError> {
        let expected_state = self.store.get_cursor(account_id, SyncObject::Message)?;
        let outcome = if let Some(gateway) = self.gateway(account_id) {
            match mutation {
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
            }
        } else {
            MutationOutcome::default()
        };

        match mutation {
            MessageMutation::SetKeywords(command) => {
                self.store
                    .set_keywords(account_id, message_id, outcome.cursor.as_ref(), command)
            }
            MessageMutation::ReplaceMailboxes(command) => self.store.replace_mailboxes(
                account_id,
                message_id,
                outcome.cursor.as_ref(),
                command,
            ),
            MessageMutation::Destroy => {
                self.store
                    .destroy_message(account_id, message_id, outcome.cursor.as_ref())
            }
        }
        .map_err(Into::into)
    }

    /// Add/remove JMAP keywords on a message.
    ///
    /// @spec spec/L1-api#message-commands
    pub async fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &SetKeywordsCommand,
    ) -> Result<CommandResult, ServiceError> {
        self.apply_message_mutation(
            account_id,
            message_id,
            MessageMutation::SetKeywords(command),
        )
        .await
    }

    /// Atomically replace all mailbox memberships for a message.
    ///
    /// @spec spec/L1-api#message-commands
    pub async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &ReplaceMailboxesCommand,
    ) -> Result<CommandResult, ServiceError> {
        self.apply_message_mutation(
            account_id,
            message_id,
            MessageMutation::ReplaceMailboxes(command),
        )
        .await
    }

    /// Add a message to a mailbox (idempotent: no-op if already present).
    ///
    /// @spec spec/L1-api#message-commands
    pub async fn add_to_mailbox(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &AddToMailboxCommand,
    ) -> Result<CommandResult, ServiceError> {
        let mut mailbox_ids = self.store.get_message_mailboxes(account_id, message_id)?;
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
    /// @spec spec/L1-api#message-commands
    pub async fn remove_from_mailbox(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &RemoveFromMailboxCommand,
    ) -> Result<CommandResult, ServiceError> {
        let mailbox_ids = self
            .store
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

    /// Permanently delete a message.
    ///
    /// @spec spec/L1-api#message-commands
    pub async fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<CommandResult, ServiceError> {
        self.apply_message_mutation(account_id, message_id, MessageMutation::Destroy)
            .await
    }

    /// Query the event log with optional filters.
    ///
    /// @spec spec/L1-api#sse-event-stream
    pub fn list_events(
        &self,
        filter: &crate::EventFilter,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        self.store.list_events(filter).map_err(Into::into)
    }

    /// Fetch the primary sender identity from the gateway.
    ///
    /// @spec spec/L1-jmap#methods-used
    pub async fn fetch_identity(&self, account_id: &AccountId) -> Result<Identity, ServiceError> {
        let gateway = self.required_gateway(account_id)?;
        gateway.fetch_identity(account_id).await.map_err(Into::into)
    }

    /// Fetch reply/forward metadata for composing a response.
    pub async fn fetch_reply_context(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<crate::ReplyContext, ServiceError> {
        let gateway = self.required_gateway(account_id)?;
        gateway
            .fetch_reply_context(account_id, message_id)
            .await
            .map_err(Into::into)
    }

    /// Send an email via the gateway.
    ///
    /// @spec spec/L1-jmap#methods-used
    pub async fn send_message(
        &self,
        account_id: &AccountId,
        request: &SendMessageRequest,
    ) -> Result<(), ServiceError> {
        let gateway = self.required_gateway(account_id)?;
        gateway
            .send_message(account_id, request)
            .await
            .map_err(Into::into)
    }

    /// Look up a gateway for an account, returning `None` if none is registered.
    fn gateway(&self, account_id: &AccountId) -> Option<SharedGateway> {
        self.gateways
            .read()
            .expect("gateway registry lock poisoned")
            .get(account_id.as_str())
            .cloned()
    }

    /// Look up a gateway, returning `GatewayError::Unavailable` if none is registered.
    fn required_gateway(&self, account_id: &AccountId) -> Result<SharedGateway, ServiceError> {
        self.gateways
            .read()
            .expect("gateway registry lock poisoned")
            .get(account_id.as_str())
            .cloned()
            .ok_or_else(|| crate::GatewayError::Unavailable(account_id.to_string()).into())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use super::*;
    use crate::{
        ConfigError, ConfigSnapshot, DomainEvent, EventFilter, FetchedBody, GatewayError,
        MessageDetail, MutationOutcome, PushTransport, SmartMailboxCondition, SmartMailboxField,
        SmartMailboxGroup, SmartMailboxGroupOperator, SmartMailboxKind, SmartMailboxOperator,
        SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue, StoreError, SyncBatch,
        SyncCursor,
    };

    #[derive(Default)]
    struct TestConfig {
        smart_mailboxes: Vec<SmartMailbox>,
        sources: Vec<AccountSettings>,
    }

    impl ConfigRepository for TestConfig {
        fn load_snapshot(&self) -> Result<ConfigSnapshot, ConfigError> {
            Ok(ConfigSnapshot {
                app_settings: AppSettings::default(),
                sources: self.sources.clone(),
                smart_mailboxes: self.smart_mailboxes.clone(),
            })
        }

        fn reload(&self) -> Result<ConfigDiff, ConfigError> {
            Ok(ConfigDiff {
                added_sources: Vec::new(),
                changed_sources: Vec::new(),
                removed_sources: Vec::new(),
            })
        }

        fn get_app_settings(&self) -> Result<AppSettings, ConfigError> {
            Ok(AppSettings::default())
        }

        fn put_app_settings(&self, _settings: &AppSettings) -> Result<(), ConfigError> {
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

        fn delete_source(&self, _id: &AccountId) -> Result<(), ConfigError> {
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
        mutation_state: Mutex<MutationStoreState>,
    }

    impl Default for TestStore {
        fn default() -> Self {
            Self {
                smart_mailbox_counts_error: None,
                list_mailboxes_error: None,
                projection_calls: Mutex::new(Vec::new()),
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

    impl MailStore for TestStore {
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

        fn list_messages(
            &self,
            _account_id: &AccountId,
            _mailbox_id: Option<&MailboxId>,
        ) -> Result<Vec<MessageSummary>, StoreError> {
            Ok(Vec::new())
        }

        fn query_messages_by_rule(
            &self,
            _rule: &SmartMailboxRule,
        ) -> Result<Vec<MessageSummary>, StoreError> {
            Ok(Vec::new())
        }

        fn query_conversations_by_rule(
            &self,
            _rule: &SmartMailboxRule,
            _limit: usize,
            _cursor: Option<&ConversationCursor>,
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

        fn list_conversations(
            &self,
            _account_id: Option<&AccountId>,
            _mailbox_id: Option<&MailboxId>,
            _limit: usize,
            _cursor: Option<&ConversationCursor>,
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

        fn delete_source_projection(&self, _source_id: &AccountId) -> Result<(), StoreError> {
            Ok(())
        }

        fn delete_source_data(&self, _account_id: &AccountId) -> Result<(), StoreError> {
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
            driver: crate::AccountDriver::Mock,
            enabled: true,
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
        let service = MailService::new(store.clone(), config)
            .with_gateway(&account, Arc::new(MutationGateway::with_revision(1)));

        service
            .set_keywords(
                &account,
                &MessageId::from("message-1"),
                &SetKeywordsCommand {
                    add: vec!["$flagged".to_string()],
                    remove: Vec::new(),
                },
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
        let service = MailService::new(store.clone(), config)
            .with_gateway(&account, Arc::new(MutationGateway::with_revision(1)));

        service
            .set_keywords(
                &account,
                &MessageId::from("message-1"),
                &SetKeywordsCommand {
                    add: vec!["$flagged".to_string()],
                    remove: Vec::new(),
                },
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
        let service = MailService::new(store.clone(), config)
            .with_gateway(&account, Arc::new(MutationGateway::with_revision(2)));

        let error = service
            .set_keywords(
                &account,
                &MessageId::from("message-1"),
                &SetKeywordsCommand {
                    add: vec!["$flagged".to_string()],
                    remove: Vec::new(),
                },
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
}
