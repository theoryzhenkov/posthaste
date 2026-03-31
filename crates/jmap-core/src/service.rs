use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::json;

use crate::{
    AccountId, AccountSettings, AddToMailboxCommand, AppSettings, CommandResult, ConfigDiff,
    ConfigRepository, ConversationCursor, ConversationId, ConversationPage, ConversationView,
    Identity, MailGateway, MailStore, MailboxId, MailboxSummary, MessageId, MessageSummary,
    RemoveFromMailboxCommand, ReplaceMailboxesCommand, SendMessageRequest, ServiceError,
    SetKeywordsCommand, SharedConfigRepository, SharedGateway, SharedStore, SidebarResponse,
    SidebarSmartMailbox, SidebarSource, SmartMailbox, SmartMailboxId, SmartMailboxSummary,
    SyncObject, SyncTrigger, ThreadId, ThreadView, EVENT_TOPIC_SYNC_COMPLETED,
    EVENT_TOPIC_SYNC_FAILED,
};
use crate::{DomainEvent, ServiceResultExt};

pub struct MailService {
    store: SharedStore,
    config: SharedConfigRepository,
    gateways: RwLock<HashMap<String, SharedGateway>>,
}

impl MailService {
    pub fn new(store: Arc<dyn MailStore>, config: Arc<dyn ConfigRepository>) -> Self {
        Self {
            store,
            config,
            gateways: RwLock::new(HashMap::new()),
        }
    }

    pub fn with_gateway(mut self, account_id: &AccountId, gateway: Arc<dyn MailGateway>) -> Self {
        self.gateways
            .get_mut()
            .expect("gateway registry lock poisoned")
            .insert(account_id.to_string(), gateway);
        self
    }

    pub fn set_gateway(&self, account_id: &AccountId, gateway: SharedGateway) {
        self.gateways
            .write()
            .expect("gateway registry lock poisoned")
            .insert(account_id.to_string(), gateway);
    }

    pub fn remove_gateway(&self, account_id: &AccountId) {
        self.gateways
            .write()
            .expect("gateway registry lock poisoned")
            .remove(account_id.as_str());
    }

    // -- Config delegates --

    pub fn get_app_settings(&self) -> Result<AppSettings, ServiceError> {
        self.config.get_app_settings().map_err(Into::into)
    }

    pub fn put_app_settings(&self, settings: &AppSettings) -> Result<(), ServiceError> {
        self.config.put_app_settings(settings).map_err(Into::into)
    }

    pub fn list_sources(&self) -> Result<Vec<AccountSettings>, ServiceError> {
        self.config.list_sources().map_err(Into::into)
    }

    pub fn get_source(&self, id: &AccountId) -> Result<Option<AccountSettings>, ServiceError> {
        self.config.get_source(id).map_err(Into::into)
    }

    pub fn save_source(&self, source: &AccountSettings) -> Result<(), ServiceError> {
        self.config.save_source(source)?;
        self.store
            .upsert_source_projection(&source.id, &source.name)?;
        Ok(())
    }

    pub fn delete_source(&self, id: &AccountId) -> Result<(), ServiceError> {
        self.config.delete_source(id)?;
        self.store.delete_source_projection(id)?;
        self.store.delete_source_data(id)?;
        Ok(())
    }

    pub fn list_smart_mailboxes_config(&self) -> Result<Vec<SmartMailbox>, ServiceError> {
        self.config.list_smart_mailboxes().map_err(Into::into)
    }

    pub fn get_smart_mailbox(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<SmartMailbox, ServiceError> {
        self.config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())
    }

    pub fn save_smart_mailbox(&self, smart_mailbox: &SmartMailbox) -> Result<(), ServiceError> {
        self.config
            .save_smart_mailbox(smart_mailbox)
            .map_err(Into::into)
    }

    pub fn delete_smart_mailbox(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<(), ServiceError> {
        self.config
            .delete_smart_mailbox(smart_mailbox_id)
            .map_err(Into::into)
    }

    pub fn reset_default_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ServiceError> {
        self.config
            .reset_default_smart_mailboxes()
            .map_err(Into::into)
    }

    pub fn reload_config(&self) -> Result<ConfigDiff, ServiceError> {
        let diff = self.config.reload()?;
        // Sync all source projections after reload
        self.sync_source_projections()?;
        Ok(diff)
    }

    pub fn sync_source_projections(&self) -> Result<(), ServiceError> {
        let sources = self.config.list_sources()?;
        for source in &sources {
            self.store
                .upsert_source_projection(&source.id, &source.name)?;
        }
        Ok(())
    }

    // -- Composed queries (config + store) --

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

    pub fn list_mailboxes(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<MailboxSummary>, ServiceError> {
        self.store.list_mailboxes(account_id).map_err(Into::into)
    }

    pub fn list_messages(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        self.store
            .list_messages(account_id, mailbox_id)
            .map_err(Into::into)
    }

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

    pub fn get_conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<ConversationView, ServiceError> {
        self.store
            .get_conversation(conversation_id)?
            .not_found("conversation", conversation_id.as_str())
    }

    pub fn get_thread(
        &self,
        account_id: &AccountId,
        thread_id: &ThreadId,
    ) -> Result<ThreadView, ServiceError> {
        self.store
            .get_thread(account_id, thread_id)?
            .not_found("thread", thread_id.as_str())
    }

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

    pub async fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &SetKeywordsCommand,
    ) -> Result<CommandResult, ServiceError> {
        let expected_state = self.store.get_cursor(account_id, SyncObject::Message)?;
        if let Some(gateway) = self.gateway(account_id) {
            gateway
                .set_keywords(
                    account_id,
                    message_id,
                    expected_state.as_ref().map(|cursor| cursor.state.as_str()),
                    command,
                )
                .await?;
        }
        self.store
            .set_keywords(account_id, message_id, command)
            .map_err(Into::into)
    }

    pub async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &ReplaceMailboxesCommand,
    ) -> Result<CommandResult, ServiceError> {
        let expected_state = self.store.get_cursor(account_id, SyncObject::Message)?;
        if let Some(gateway) = self.gateway(account_id) {
            gateway
                .replace_mailboxes(
                    account_id,
                    message_id,
                    expected_state.as_ref().map(|cursor| cursor.state.as_str()),
                    &command.mailbox_ids,
                )
                .await?;
        }
        self.store
            .replace_mailboxes(account_id, message_id, command)
            .map_err(Into::into)
    }

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

    pub async fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<CommandResult, ServiceError> {
        let expected_state = self.store.get_cursor(account_id, SyncObject::Message)?;
        if let Some(gateway) = self.gateway(account_id) {
            gateway
                .destroy_message(
                    account_id,
                    message_id,
                    expected_state.as_ref().map(|cursor| cursor.state.as_str()),
                )
                .await?;
        }
        self.store
            .destroy_message(account_id, message_id)
            .map_err(Into::into)
    }

    pub fn list_events(
        &self,
        filter: &crate::EventFilter,
    ) -> Result<Vec<DomainEvent>, ServiceError> {
        self.store.list_events(filter).map_err(Into::into)
    }

    pub async fn fetch_identity(&self, account_id: &AccountId) -> Result<Identity, ServiceError> {
        let gateway = self.required_gateway(account_id)?;
        gateway.fetch_identity(account_id).await.map_err(Into::into)
    }

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

    fn gateway(&self, account_id: &AccountId) -> Option<SharedGateway> {
        self.gateways
            .read()
            .expect("gateway registry lock poisoned")
            .get(account_id.as_str())
            .cloned()
    }

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

    use super::*;
    use crate::{
        ConfigError, ConfigSnapshot, DomainEvent, EventFilter, FetchedBody, MessageDetail,
        SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
        SmartMailboxKind, SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode,
        SmartMailboxValue, StoreError, SyncBatch, SyncCursor,
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
    }

    impl Default for TestStore {
        fn default() -> Self {
            Self {
                smart_mailbox_counts_error: None,
                list_mailboxes_error: None,
                projection_calls: Mutex::new(Vec::new()),
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
            _object_type: SyncObject,
        ) -> Result<Option<SyncCursor>, StoreError> {
            Ok(None)
        }

        fn get_message_mailboxes(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
        ) -> Result<Vec<MailboxId>, StoreError> {
            Ok(Vec::new())
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
            _command: &SetKeywordsCommand,
        ) -> Result<CommandResult, StoreError> {
            Err(StoreError::Failure("unused".to_string()))
        }

        fn replace_mailboxes(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            _command: &ReplaceMailboxesCommand,
        ) -> Result<CommandResult, StoreError> {
            Err(StoreError::Failure("unused".to_string()))
        }

        fn destroy_message(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
        ) -> Result<CommandResult, StoreError> {
            Err(StoreError::Failure("unused".to_string()))
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
}
