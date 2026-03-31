use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::json;

use crate::{
    AccountId, AddToMailboxCommand, CommandResult, ConversationId, ConversationSummary,
    ConversationView, Identity, MailGateway, MailStore, MailboxId, MailboxSummary, MessageId,
    MessageSummary, RemoveFromMailboxCommand, ReplaceMailboxesCommand, ReplyContext,
    SendMessageRequest, ServiceError, SetKeywordsCommand, SharedGateway, SharedStore,
    SidebarResponse, SmartMailbox, SmartMailboxId, SmartMailboxSummary, SyncObject, SyncTrigger,
    ThreadId, ThreadView,
};
use crate::{DomainEvent, ServiceResultExt};

pub struct MailService {
    store: SharedStore,
    gateways: RwLock<HashMap<String, SharedGateway>>,
}

impl MailService {
    pub fn new(store: Arc<dyn MailStore>) -> Self {
        Self {
            store,
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

    pub fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailboxSummary>, ServiceError> {
        self.store.list_smart_mailboxes().map_err(Into::into)
    }

    pub fn get_smart_mailbox(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<SmartMailbox, ServiceError> {
        self.store
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())
    }

    pub fn create_smart_mailbox(
        &self,
        smart_mailbox: &SmartMailbox,
    ) -> Result<(), ServiceError> {
        self.store
            .create_smart_mailbox(smart_mailbox)
            .map_err(Into::into)
    }

    pub fn update_smart_mailbox(
        &self,
        smart_mailbox: &SmartMailbox,
    ) -> Result<(), ServiceError> {
        self.store
            .update_smart_mailbox(smart_mailbox)
            .map_err(Into::into)
    }

    pub fn delete_smart_mailbox(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<(), ServiceError> {
        self.store
            .delete_smart_mailbox(smart_mailbox_id)
            .map_err(Into::into)
    }

    pub fn reset_default_smart_mailboxes(
        &self,
    ) -> Result<Vec<SmartMailboxSummary>, ServiceError> {
        self.store
            .reset_default_smart_mailboxes()
            .map_err(Into::into)
    }

    pub fn list_smart_mailbox_messages(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        self.store
            .list_smart_mailbox_messages(smart_mailbox_id)
            .map_err(Into::into)
    }

    pub fn list_conversations(
        &self,
        account_id: Option<&AccountId>,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<ConversationSummary>, ServiceError> {
        self.store
            .list_conversations(account_id, mailbox_id)
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

    pub fn get_sidebar(&self) -> Result<SidebarResponse, ServiceError> {
        self.store.get_sidebar().map_err(Into::into)
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
            "sync.completed",
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
                "sync.failed",
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
    ) -> Result<ReplyContext, ServiceError> {
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

    use async_trait::async_trait;
    use serde_json::json;

        use crate::{
        AccountId, CommandResult, ConversationId, ConversationSummary, ConversationView,
        DomainEvent, EventFilter, FetchedBody, GatewayError, MailGateway, MailStore, MailboxId,
        MailboxSummary, MessageDetail, MessageId, MessageSummary, ReplaceMailboxesCommand,
        ServiceError, SetKeywordsCommand, SidebarResponse, SmartMailbox, SmartMailboxId,
        SmartMailboxSummary, SyncBatch, SyncCursor, SyncObject, ThreadId, ThreadView,
    };

    use super::MailService;

    struct TestStore {
        mailboxes: Mutex<Vec<MailboxId>>,
    }

    #[async_trait]
    impl MailStore for TestStore {
        fn list_mailboxes(
            &self,
            _account_id: &AccountId,
        ) -> Result<Vec<MailboxSummary>, crate::StoreError> {
            Ok(Vec::new())
        }

        fn list_messages(
            &self,
            _account_id: &AccountId,
            _mailbox_id: Option<&MailboxId>,
        ) -> Result<Vec<MessageSummary>, crate::StoreError> {
            Ok(Vec::new())
        }

        fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailboxSummary>, crate::StoreError> {
            Ok(Vec::new())
        }

        fn get_smart_mailbox(
            &self,
            _smart_mailbox_id: &SmartMailboxId,
        ) -> Result<Option<SmartMailbox>, crate::StoreError> {
            Ok(None)
        }

        fn create_smart_mailbox(
            &self,
            _smart_mailbox: &SmartMailbox,
        ) -> Result<(), crate::StoreError> {
            Ok(())
        }

        fn update_smart_mailbox(
            &self,
            _smart_mailbox: &SmartMailbox,
        ) -> Result<(), crate::StoreError> {
            Ok(())
        }

        fn delete_smart_mailbox(
            &self,
            _smart_mailbox_id: &SmartMailboxId,
        ) -> Result<(), crate::StoreError> {
            Ok(())
        }

        fn reset_default_smart_mailboxes(
            &self,
        ) -> Result<Vec<SmartMailboxSummary>, crate::StoreError> {
            Ok(Vec::new())
        }

        fn list_smart_mailbox_messages(
            &self,
            _smart_mailbox_id: &SmartMailboxId,
        ) -> Result<Vec<MessageSummary>, crate::StoreError> {
            Ok(Vec::new())
        }

        fn get_message_detail(
            &self,
            _account_id: &AccountId,
            message_id: &MessageId,
        ) -> Result<Option<MessageDetail>, crate::StoreError> {
            Ok(Some(MessageDetail {
                summary: MessageSummary {
                    id: message_id.clone(),
                    source_id: AccountId::from("primary"),
                    source_name: "Primary".to_string(),
                    source_thread_id: ThreadId::from("thread-1"),
                    conversation_id: ConversationId::from("conversation-1"),
                    subject: None,
                    from_name: None,
                    from_email: None,
                    preview: None,
                    received_at: "2026-03-31T00:00:00Z".to_string(),
                    has_attachment: false,
                    is_read: false,
                    is_flagged: false,
                    mailbox_ids: self.mailboxes.lock().unwrap().clone(),
                    keywords: Vec::new(),
                },
                body_html: Some("<p>ready</p>".to_string()),
                body_text: None,
                raw_message: None,
            }))
        }

        fn get_thread(
            &self,
            _account_id: &AccountId,
            _thread_id: &ThreadId,
        ) -> Result<Option<ThreadView>, crate::StoreError> {
            Ok(None)
        }

        fn list_conversations(
            &self,
            _account_id: Option<&AccountId>,
            _mailbox_id: Option<&MailboxId>,
        ) -> Result<Vec<ConversationSummary>, crate::StoreError> {
            Ok(Vec::new())
        }

        fn get_conversation(
            &self,
            _conversation_id: &ConversationId,
        ) -> Result<Option<ConversationView>, crate::StoreError> {
            Ok(None)
        }

        fn get_sidebar(&self) -> Result<SidebarResponse, crate::StoreError> {
            Ok(SidebarResponse {
                smart_mailboxes: Vec::new(),
                sources: Vec::new(),
            })
        }

        fn get_sync_cursors(
            &self,
            _account_id: &AccountId,
        ) -> Result<Vec<SyncCursor>, crate::StoreError> {
            Ok(Vec::new())
        }

        fn get_cursor(
            &self,
            _account_id: &AccountId,
            _object_type: SyncObject,
        ) -> Result<Option<SyncCursor>, crate::StoreError> {
            Ok(Some(SyncCursor {
                object_type: SyncObject::Message,
                state: "1".to_string(),
                updated_at: "2026-03-31T00:00:00Z".to_string(),
            }))
        }

        fn get_message_mailboxes(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
        ) -> Result<Vec<MailboxId>, crate::StoreError> {
            Ok(self.mailboxes.lock().unwrap().clone())
        }

        fn apply_sync_batch(
            &self,
            _account_id: &AccountId,
            _batch: &SyncBatch,
        ) -> Result<Vec<DomainEvent>, crate::StoreError> {
            Ok(Vec::new())
        }

        fn apply_message_body(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            _body: &FetchedBody,
        ) -> Result<CommandResult, crate::StoreError> {
            Err(crate::StoreError::Failure("unused".to_string()))
        }

        fn set_keywords(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            _command: &SetKeywordsCommand,
        ) -> Result<CommandResult, crate::StoreError> {
            Err(crate::StoreError::Failure("unused".to_string()))
        }

        fn replace_mailboxes(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            command: &ReplaceMailboxesCommand,
        ) -> Result<CommandResult, crate::StoreError> {
            *self.mailboxes.lock().unwrap() = command.mailbox_ids.clone();
            Ok(CommandResult {
                detail: None,
                events: Vec::new(),
            })
        }

        fn destroy_message(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
        ) -> Result<CommandResult, crate::StoreError> {
            Err(crate::StoreError::Failure("unused".to_string()))
        }

        fn list_events(
            &self,
            _filter: &EventFilter,
        ) -> Result<Vec<DomainEvent>, crate::StoreError> {
            Ok(Vec::new())
        }

        fn append_event(
            &self,
            _account_id: &AccountId,
            _topic: &str,
            _mailbox_id: Option<&MailboxId>,
            _message_id: Option<&MessageId>,
            _payload: serde_json::Value,
        ) -> Result<DomainEvent, crate::StoreError> {
            Ok(DomainEvent {
                seq: 1,
                account_id: AccountId::from("primary"),
                topic: "sync.completed".to_string(),
                occurred_at: "2026-03-31T00:00:00Z".to_string(),
                mailbox_id: None,
                message_id: None,
                payload: json!({}),
            })
        }
    }

    struct TestGateway;

    #[async_trait]
    impl MailGateway for TestGateway {
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
            _expected_state: Option<&str>,
            _command: &SetKeywordsCommand,
        ) -> Result<(), GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn replace_mailboxes(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            _expected_state: Option<&str>,
            _mailbox_ids: &[MailboxId],
        ) -> Result<(), GatewayError> {
            Ok(())
        }

        async fn destroy_message(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            _expected_state: Option<&str>,
        ) -> Result<(), GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn fetch_identity(
            &self,
            _account_id: &AccountId,
        ) -> Result<crate::Identity, GatewayError> {
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
            _request: &crate::SendMessageRequest,
        ) -> Result<(), GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn open_push_stream(
            &self,
            _account_id: &AccountId,
            _last_event_id: Option<&str>,
        ) -> Result<Option<crate::PushStream>, GatewayError> {
            Ok(None)
        }
    }

    struct StateMismatchGateway;

    #[async_trait]
    impl MailGateway for StateMismatchGateway {
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
            _expected_state: Option<&str>,
            _command: &SetKeywordsCommand,
        ) -> Result<(), GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn replace_mailboxes(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            _expected_state: Option<&str>,
            _mailbox_ids: &[MailboxId],
        ) -> Result<(), GatewayError> {
            Err(GatewayError::StateMismatch)
        }

        async fn destroy_message(
            &self,
            _account_id: &AccountId,
            _message_id: &MessageId,
            _expected_state: Option<&str>,
        ) -> Result<(), GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn fetch_identity(
            &self,
            _account_id: &AccountId,
        ) -> Result<crate::Identity, GatewayError> {
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
            _request: &crate::SendMessageRequest,
        ) -> Result<(), GatewayError> {
            Err(GatewayError::Rejected("unused".to_string()))
        }

        async fn open_push_stream(
            &self,
            _account_id: &AccountId,
            _last_event_id: Option<&str>,
        ) -> Result<Option<crate::PushStream>, GatewayError> {
            Ok(None)
        }
    }

    #[tokio::test]
    async fn add_to_mailbox_preserves_existing_membership() -> Result<(), ServiceError> {
        let account_id = AccountId::from("primary");
        let store = Arc::new(TestStore {
            mailboxes: Mutex::new(vec![MailboxId::from("inbox")]),
        });
        let service = MailService::new(store).with_gateway(&account_id, Arc::new(TestGateway));

        service
            .add_to_mailbox(
                &account_id,
                &MessageId::from("message-1"),
                &crate::AddToMailboxCommand {
                    mailbox_id: MailboxId::from("archive"),
                },
            )
            .await?;

        let updated = service
            .store
            .get_message_mailboxes(&account_id, &MessageId::from("message-1"))?;
        assert_eq!(
            updated,
            vec![MailboxId::from("inbox"), MailboxId::from("archive")]
        );
        Ok(())
    }

    #[tokio::test]
    async fn replace_mailboxes_surfaces_state_mismatch_without_local_mutation(
    ) -> Result<(), ServiceError> {
        let account_id = AccountId::from("primary");
        let store = Arc::new(TestStore {
            mailboxes: Mutex::new(vec![MailboxId::from("inbox")]),
        });
        let service = MailService::new(store.clone())
            .with_gateway(&account_id, Arc::new(StateMismatchGateway));

        let error = service
            .replace_mailboxes(
                &account_id,
                &MessageId::from("message-1"),
                &ReplaceMailboxesCommand {
                    mailbox_ids: vec![MailboxId::from("archive")],
                },
            )
            .await
            .expect_err("expected state mismatch");

        assert_eq!(error.code(), "state_mismatch");
        assert_eq!(
            store.get_message_mailboxes(&account_id, &MessageId::from("message-1"))?,
            vec![MailboxId::from("inbox")]
        );
        Ok(())
    }
}
