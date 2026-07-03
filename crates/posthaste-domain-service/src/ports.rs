use std::sync::Arc;

use crate::PushTransport;
use posthaste_domain_model::{
    AccountId, AutomationBackfillJob, BlobId, CacheCandidate, CacheFetchCandidate, CacheLayer,
    CacheObjectState, CachePriorityUpdate, CacheRescoreCandidate, CacheSignalUpdate,
    CachedSenderAddress, CommandResult, ConversationCursor, ConversationId, ConversationPage,
    ConversationSortField, ConversationView, EventFilter, FetchedBody, Identity,
    ImapMailboxSyncState, ImapMessageLocation, MailboxId, MailboxSummary, MessageCursor,
    MessageDetail, MessageId, MessagePage, MessageSortField, MessageSummary, MutationOutcome,
    Operation, OperationId, OperationState, Recipient, ReplaceMailboxesCommand, ReplyContext,
    RevLogSnapshot, SecretRef, SecretStoreError, SendMessageRequest, SetKeywordsCommand,
    SmartMailboxRule, SortDirection, SyncBatch, SyncCursor, SyncObject, SyncOutcome, SyncProgress,
    SyncReconciliation, SyncTrigger, TagSummary, ThreadId, ThreadView,
};
use posthaste_domain_model::{
    DomainEvent, EventLogBounds, GatewayError, ServiceError, StoreError,
};

mod cache_store;
mod composite;
mod gateway;
mod progress;
mod read_store;
mod sync_store;
mod write_store;

pub use cache_store::CacheStore;
pub use composite::{MailStore, SecretStore, ServiceResultExt, SharedGateway, SharedSecretStore};
pub use gateway::{MailGateway, SyncChunkSink};
pub use progress::SyncProgressReporter;
pub use read_store::{
    ConversationReadStore, MailboxReadStore, MailboxRoleOverrideStore, MessageDetailStore,
    MessageListStore, RevLogStore, SmartMailboxStore, SnoozeStore, TagReadStore,
};
pub use sync_store::{
    ImapMessageLocationStore, ImapMessageLocationWriteStore, ImapSyncStateStore,
    ImapSyncStateWriteStore, MessageMailboxStore, SyncStateStore, SyncWriteStore,
};
pub use write_store::{
    AutomationBackfillStore, EventStore, MessageCommandStore, OperationOutboxStore,
    SenderAddressCacheStore, SourceDataStore, SourceProjectionStore,
};
