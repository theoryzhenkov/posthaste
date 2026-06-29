use std::sync::Arc;

use posthaste_observability::{events, ph_warn};
use serde_json::json;

use crate::{now_iso8601, GatewayError, Id};
use crate::{
    AccountId, AccountSettings, AppSettings, AutomationBackfillStore, CacheStore, CommandResult,
    ConfigDiff, ConfigRepository, ConversationCursor, ConversationId, ConversationPage,
    ConversationReadStore, ConversationSortField, ConversationView, DraftContent,
    DraftContentResult, EventStore, Identity, MailGateway, MailStore, MailboxId, MailboxReadStore,
    MailboxSummary, MessageCommandStore, MessageCursor, MessageDetail, MessageDetailStore,
    MessageId, MessageListStore, MessageMailboxStore, MessagePage, MessageSortField,
    MessageSummary, Operation, OperationEntity, OperationEntityKind, OperationId, OperationKind,
    OperationOutboxStore, OperationOutcome, OperationSettlement, OperationState, Recipient,
    ReplaceMailboxesCommand, SendMessageRequest, ServiceError, SetKeywordsCommand,
    SharedConfigRepository, SmartMailbox, SmartMailboxId, SmartMailboxRule, SmartMailboxStore,
    SmartMailboxSummary, SnoozeStore, SortDirection, SourceDataStore, SourceProjectionStore,
    StoreError, SyncMode, SyncObject, SyncStateStore, SyncTrigger, SyncWriteStore, TagReadStore,
    TagSummary, ThreadId, ThreadView, EVENT_TOPIC_MESSAGE_UPDATED, EVENT_TOPIC_OPERATION_SETTLED,
    EVENT_TOPIC_SYNC_COMPLETED, EVENT_TOPIC_SYNC_FAILED,
};
use crate::{DomainEvent, ServiceResultExt};

mod automation;
mod cache;
mod config_delegates;
mod gateway_ops;
mod mailbox_queries;
mod message_queries;
mod mutation;
mod outbox;
mod smart_mailbox_queries;
mod sync_ops;
#[cfg(test)]
mod tests;

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
    tag_reader: Arc<dyn TagReadStore>,
    conversation_reader: Arc<dyn ConversationReadStore>,
    message_detail_reader: Arc<dyn MessageDetailStore>,
    smart_mailboxes: Arc<dyn SmartMailboxStore>,
    sync_state: Arc<dyn SyncStateStore>,
    message_mailboxes: Arc<dyn MessageMailboxStore>,
    message_commands: Arc<dyn MessageCommandStore>,
    sync_writer: Arc<dyn SyncWriteStore>,
    events: Arc<dyn EventStore>,
    source_projections: Arc<dyn SourceProjectionStore>,
    source_data: Arc<dyn SourceDataStore>,
    cache_store: Arc<dyn CacheStore>,
    automation_backfills: Arc<dyn AutomationBackfillStore>,
    snooze_reader: Arc<dyn SnoozeStore>,
    outbox: Arc<dyn OperationOutboxStore>,
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
            tag_reader: store.clone(),
            conversation_reader: store.clone(),
            message_detail_reader: store.clone(),
            smart_mailboxes: store.clone(),
            sync_state: store.clone(),
            message_mailboxes: store.clone(),
            message_commands: store.clone(),
            sync_writer: store.clone(),
            events: store.clone(),
            source_projections: store.clone(),
            source_data: store.clone(),
            cache_store: store.clone(),
            automation_backfills: store.clone(),
            snooze_reader: store.clone(),
            outbox: store,
        }
    }
}
