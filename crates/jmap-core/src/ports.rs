use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    AccountId, AccountSettings, AppSettings, CommandResult, EventFilter, FetchedBody, Identity,
    MailboxId, MailboxSummary, MessageDetail, MessageId, MessageSummary, PushStream,
    ReplaceMailboxesCommand, ReplyContext, SecretRef, SecretStoreError, SendMessageRequest,
    SetKeywordsCommand, SyncBatch, SyncCursor, SyncObject, ThreadId, ThreadView,
};
use crate::{DomainEvent, GatewayError, ServiceError, StoreError};

#[async_trait]
pub trait MailGateway: Send + Sync {
    async fn sync(
        &self,
        account_id: &AccountId,
        cursors: &[SyncCursor],
    ) -> Result<SyncBatch, GatewayError>;
    async fn fetch_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError>;
    async fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<(), GatewayError>;
    async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<(), GatewayError>;
    async fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
    ) -> Result<(), GatewayError>;
    async fn fetch_identity(&self, account_id: &AccountId) -> Result<Identity, GatewayError>;
    async fn fetch_reply_context(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError>;
    async fn send_message(
        &self,
        account_id: &AccountId,
        request: &SendMessageRequest,
    ) -> Result<(), GatewayError>;
    async fn open_push_stream(
        &self,
        account_id: &AccountId,
        last_event_id: Option<&str>,
    ) -> Result<Option<PushStream>, GatewayError>;
}

#[async_trait]
pub trait MailStore: Send + Sync {
    fn list_mailboxes(&self, account_id: &AccountId) -> Result<Vec<MailboxSummary>, StoreError>;
    fn list_messages(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, StoreError>;
    fn get_message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, StoreError>;
    fn get_thread(
        &self,
        account_id: &AccountId,
        thread_id: &ThreadId,
    ) -> Result<Option<ThreadView>, StoreError>;
    fn get_sync_cursors(&self, account_id: &AccountId) -> Result<Vec<SyncCursor>, StoreError>;
    fn get_cursor(
        &self,
        account_id: &AccountId,
        object_type: SyncObject,
    ) -> Result<Option<SyncCursor>, StoreError>;
    fn get_message_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Vec<MailboxId>, StoreError>;
    fn apply_sync_batch(
        &self,
        account_id: &AccountId,
        batch: &SyncBatch,
    ) -> Result<Vec<DomainEvent>, StoreError>;
    fn apply_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        body: &FetchedBody,
    ) -> Result<CommandResult, StoreError>;
    fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &SetKeywordsCommand,
    ) -> Result<CommandResult, StoreError>;
    fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        command: &ReplaceMailboxesCommand,
    ) -> Result<CommandResult, StoreError>;
    fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<CommandResult, StoreError>;
    fn list_events(&self, filter: &EventFilter) -> Result<Vec<DomainEvent>, StoreError>;
    fn append_event(
        &self,
        account_id: &AccountId,
        topic: &str,
        mailbox_id: Option<&MailboxId>,
        message_id: Option<&MessageId>,
        payload: serde_json::Value,
    ) -> Result<DomainEvent, StoreError>;
    fn get_app_settings(&self) -> Result<AppSettings, StoreError> {
        Ok(AppSettings::default())
    }
    fn put_app_settings(&self, _settings: &AppSettings) -> Result<(), StoreError> {
        Err(StoreError::Failure("settings not supported".to_string()))
    }
    fn list_accounts(&self) -> Result<Vec<AccountSettings>, StoreError> {
        Ok(Vec::new())
    }
    fn get_account(&self, _account_id: &AccountId) -> Result<Option<AccountSettings>, StoreError> {
        Ok(None)
    }
    fn create_account(&self, _account: &AccountSettings) -> Result<(), StoreError> {
        Err(StoreError::Failure("accounts not supported".to_string()))
    }
    fn update_account(&self, _account: &AccountSettings) -> Result<(), StoreError> {
        Err(StoreError::Failure("accounts not supported".to_string()))
    }
    fn delete_account(&self, _account_id: &AccountId) -> Result<(), StoreError> {
        Err(StoreError::Failure("accounts not supported".to_string()))
    }
}

pub trait SecretStore: Send + Sync {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<String, SecretStoreError>;
    fn save(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError>;
    fn update(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError>;
    fn delete(&self, secret_ref: &SecretRef) -> Result<(), SecretStoreError>;
}

pub type SharedStore = Arc<dyn MailStore>;
pub type SharedGateway = Arc<dyn MailGateway>;
pub type SharedSecretStore = Arc<dyn SecretStore>;

pub trait ServiceResultExt<T> {
    fn not_found(self, kind: &str, id: &str) -> Result<T, ServiceError>;
}

impl<T> ServiceResultExt<T> for Option<T> {
    fn not_found(self, kind: &str, id: &str) -> Result<T, ServiceError> {
        self.ok_or_else(|| StoreError::NotFound(format!("{kind}:{id}")).into())
    }
}
