use std::sync::Arc;

use crate::{
    AccountId, AutomationBackfillJob, BlobId, CacheCandidate, CacheFetchCandidate, CacheLayer,
    CacheObjectState, CachePriorityUpdate, CacheRescoreCandidate, CacheSignalUpdate,
    CachedSenderAddress, CommandResult, ConversationCursor, ConversationId, ConversationPage,
    ConversationSortField, ConversationView, EventFilter, FetchedBody, Identity,
    ImapMailboxSyncState, ImapMessageLocation, MailboxId, MailboxSummary, MessageCursor,
    MessageDetail, MessageId, MessagePage, MessageSortField, MessageSummary, MutationOutcome,
    PushTransport, Recipient, ReplaceMailboxesCommand, ReplyContext, SecretRef, SecretStoreError,
    SendMessageRequest, SetKeywordsCommand, SmartMailboxRule, SortDirection, SyncBatch, SyncCursor,
    SyncObject, SyncProgress, SyncTrigger, TagSummary, ThreadId, ThreadView,
};
use crate::{DomainEvent, GatewayError, ServiceError, StoreError};

mod cache_store;
mod composite;
mod gateway;
mod progress;
mod read_store;
mod sync_store;
mod write_store;

pub use cache_store::CacheStore;
pub use composite::{MailStore, SecretStore, ServiceResultExt, SharedGateway, SharedSecretStore};
pub use gateway::MailGateway;
pub use progress::SyncProgressReporter;
pub use read_store::{
    ConversationReadStore, MailboxReadStore, MailboxRoleOverrideStore, MessageDetailStore,
    MessageListStore, SmartMailboxStore, TagReadStore,
};
pub use sync_store::{
    ImapMessageLocationStore, ImapMessageLocationWriteStore, ImapSyncStateStore,
    ImapSyncStateWriteStore, MessageMailboxStore, SyncStateStore, SyncWriteStore,
};
pub use write_store::{
    AutomationBackfillStore, EventStore, MessageCommandStore, SenderAddressCacheStore,
    SourceDataStore, SourceProjectionStore,
};
