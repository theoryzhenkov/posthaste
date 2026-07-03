use std::sync::Arc;

use posthaste_observability::{events, ph_warn};
use serde_json::json;

use crate::{
    AutomationBackfillStore, CacheStore, ConfigDiff, ConfigRepository, ConversationReadStore,
    EventStore, MailGateway, MailStore, MailboxReadStore, MailboxRoleOverrideStore,
    MessageCommandStore, MessageDetailStore, MessageListStore, MessageMailboxStore,
    OperationOutboxStore, ServiceResultExt, SharedConfigRepository, SmartMailboxStore, SnoozeStore,
    SourceDataStore, SourceProjectionStore, SyncStateStore, SyncWriteStore, TagReadStore,
};
use posthaste_domain_model::{
    now_iso8601, AccountId, AccountSettings, AppSettings, CommandResult, ConversationCursor,
    ConversationId, ConversationPage, ConversationSortField, ConversationView, DomainEvent,
    DraftContent, DraftContentResult, GatewayError, Id, Identity, MailboxId, MailboxSummary,
    MessageCursor, MessageDetail, MessageId, MessagePage, MessageSortField, MessageSummary,
    Operation, OperationEntity, OperationEntityKind, OperationId, OperationKind, OperationOutcome,
    OperationSettlement, OperationState, Recipient, ReplaceMailboxesCommand, SendMessageRequest,
    ServiceError, SetKeywordsCommand, SmartMailbox, SmartMailboxId, SmartMailboxRule,
    SmartMailboxSummary, SortDirection, StoreError, SyncMode, SyncObject, SyncTrigger, TagSummary,
    ThreadId, ThreadView, EVENT_TOPIC_MAILBOX_UPDATED, EVENT_TOPIC_MESSAGE_UPDATED,
    EVENT_TOPIC_OPERATION_SETTLED, EVENT_TOPIC_SYNC_COMPLETED, EVENT_TOPIC_SYNC_FAILED,
};

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
    mailbox_role_overrides: Arc<dyn MailboxRoleOverrideStore>,
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
            mailbox_role_overrides: store.clone(),
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

/// Runs a synchronous `SyncWriteStore`/`MessageCommandStore` call on the tokio
/// **blocking pool** via [`tokio::task::spawn_blocking`] (D63/M23b): those
/// ports stay plain `&self` sync traits (so `posthaste-store`'s own unit
/// tests keep calling them with no `Arc`/runtime — see the ports' doc
/// comments), so every *async* call site that drives one of the store's hot
/// write paths — `ServiceSyncSink::emit` (the snapshot/delta apply),
/// `apply_assertion_to_canonical`, the outbox settle write, lazy body caching
/// — wraps the call here rather than running the SQLite work inline on the
/// tokio worker thread. `f` typically closes over an `Arc::clone`d port
/// handle plus owned copies of its arguments (the port call borrows `&self`
/// and `&args`, both cheap to hand to the closure once cloned).
///
/// @spec docs/eph/RFC-L2-lifecycle-and-errors#d63
pub(crate) async fn offload<T, F>(f: F) -> Result<T, StoreError>
where
    F: FnOnce() -> Result<T, StoreError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .unwrap_or_else(|join_err| {
            Err(StoreError::Failure(format!(
                "store write task failed: {join_err}"
            )))
        })
}

/// Serialize an operation payload, mapping the JSON error to an internal-codec
/// `ServiceError`. A failure here is our own encode bug, not a provider
/// rejection, so it carries `GatewayError::Internal` (permanent, 500-class)
/// rather than the old `Rejected` mislabel (audit §2 serde-decode edge).
/// `context` names what was being serialized, e.g. `encode_payload(command,
/// "keyword command")` -> `"failed to serialize keyword command: <err>"`.
pub(crate) fn encode_payload<T: serde::Serialize>(
    value: T,
    context: &str,
) -> Result<serde_json::Value, ServiceError> {
    serde_json::to_value(value).map_err(|error| {
        ServiceError::from(GatewayError::Internal(format!(
            "failed to serialize {context}: {error}"
        )))
    })
}

/// Deserialize an operation payload, mapping the JSON error to an internal-codec
/// `ServiceError`. An un-decodable stored payload is an internal fault, not a
/// gateway rejection, so it carries `GatewayError::Internal` (audit §2
/// serde-decode edge). `context` names the payload, e.g. `decode_payload(payload,
/// "setKeywords payload")` -> `"invalid setKeywords payload: <err>"`.
pub(crate) fn decode_payload<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
    context: &str,
) -> Result<T, ServiceError> {
    serde_json::from_value(value).map_err(|error| {
        ServiceError::from(GatewayError::Internal(format!(
            "invalid {context}: {error}"
        )))
    })
}
