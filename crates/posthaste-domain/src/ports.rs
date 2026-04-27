use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    AccountId, AutomationBackfillJob, BlobId, CachedSenderAddress, CommandResult,
    ConversationCursor, ConversationId, ConversationPage, ConversationSortField, ConversationView,
    EventFilter, FetchedBody, Identity, ImapMailboxSyncState, ImapMessageLocation, MailboxId,
    MailboxSummary, MessageCursor, MessageDetail, MessageId, MessagePage, MessageSortField,
    MessageSummary, MutationOutcome, PushTransport, Recipient, ReplaceMailboxesCommand,
    ReplyContext, SecretRef, SecretStoreError, SendMessageRequest, SetKeywordsCommand,
    SmartMailboxRule, SortDirection, SyncBatch, SyncCursor, SyncObject, TagSummary, ThreadId,
    ThreadView,
};
use crate::{DomainEvent, GatewayError, ServiceError, StoreError};

/// Gateway to a remote JMAP server.
///
/// Abstracts JMAP protocol operations behind a domain-level interface.
/// Implementations: `LiveJmapGateway` for real JMAP, `MockGateway` for tests.
///
/// @spec docs/L1-jmap#methods-used
#[async_trait]
pub trait MailGateway: Send + Sync {
    /// Perform a delta or full sync for all object types using stored cursors.
    ///
    /// @spec docs/L1-sync#sync-loop
    async fn sync(
        &self,
        account_id: &AccountId,
        cursors: &[SyncCursor],
    ) -> Result<SyncBatch, GatewayError>;

    /// Lazily fetch body content for a single message.
    ///
    /// @spec docs/L1-sync#sync-loop
    async fn fetch_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError>;

    /// Download an attachment or inline blob by its JMAP `blobId`.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn download_blob(
        &self,
        account_id: &AccountId,
        blob_id: &BlobId,
    ) -> Result<Vec<u8>, GatewayError>;

    /// Update JMAP keywords on a message via `Email/set`.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError>;

    /// Atomically replace all mailbox memberships for a message via `Email/set`.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError>;

    /// Permanently delete a message via `Email/set`.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError>;

    /// Update a mailbox role via `Mailbox/set`.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn set_mailbox_role(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
        expected_state: Option<&str>,
        role: Option<&str>,
        clear_role_from: Option<&MailboxId>,
    ) -> Result<MutationOutcome, GatewayError>;

    /// Fetch the primary sender identity via `Identity/get`.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn fetch_identity(&self, account_id: &AccountId) -> Result<Identity, GatewayError>;

    /// Fetch reply/forward metadata for composing a response.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn fetch_reply_context(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError>;

    /// Send an email via `EmailSubmission/set`.
    ///
    /// @spec docs/L1-jmap#methods-used
    async fn send_message(
        &self,
        account_id: &AccountId,
        request: &SendMessageRequest,
    ) -> Result<(), GatewayError>;

    /// Return available push transports ordered by preference (WS first, then SSE).
    ///
    /// @spec docs/L2-transport#new-abstractions
    fn push_transports(&self) -> Vec<Box<dyn PushTransport>>;
}

/// Mailbox read projection for synced account navigation.
pub trait MailboxReadStore: Send + Sync {
    /// List all mailboxes for an account.
    ///
    /// @spec docs/L1-sync#sqlite-schema
    fn list_mailboxes(&self, account_id: &AccountId) -> Result<Vec<MailboxSummary>, StoreError>;
}

/// Message list projection for UI queries.
pub trait MessageListStore: Send + Sync {
    /// List messages, optionally filtered by mailbox.
    ///
    /// @spec docs/L1-sync#sqlite-schema
    fn list_messages(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, StoreError>;

    /// Paginated message list with seek-based cursors.
    ///
    /// @spec docs/L1-api#cursor-pagination
    fn list_message_page(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, StoreError>;
}

/// Tag read projection for non-system JMAP keywords.
pub trait TagReadStore: Send + Sync {
    /// List user-facing tags for one account with unread and total counts.
    ///
    /// @spec docs/L1-sync#sqlite-schema
    fn list_tags(&self, account_id: &AccountId) -> Result<Vec<TagSummary>, StoreError>;
}

/// Conversation list and detail projection for UI queries.
pub trait ConversationReadStore: Send + Sync {
    /// Paginated conversation list with seek-based cursors.
    ///
    /// @spec docs/L1-sync#conversation-pagination
    fn list_conversations(
        &self,
        account_id: Option<&AccountId>,
        mailbox_id: Option<&MailboxId>,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, StoreError>;

    /// Fetch a single conversation with all its messages.
    ///
    /// @spec docs/L1-sync#conversation-pagination
    fn get_conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<Option<ConversationView>, StoreError>;
}

/// Message detail read projection for message views and thread views.
pub trait MessageDetailStore: Send + Sync {
    /// Fetch full message detail including body content.
    ///
    /// @spec docs/L1-sync#body-lazy
    fn get_message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, StoreError>;

    /// Fetch all messages in a thread.
    ///
    /// @spec docs/L1-sync#sqlite-schema
    fn get_thread(
        &self,
        account_id: &AccountId,
        thread_id: &ThreadId,
    ) -> Result<Option<ThreadView>, StoreError>;
}

/// Smart mailbox rule evaluation over synced mail projections.
pub trait SmartMailboxStore: Send + Sync {
    /// Query messages matching a smart mailbox rule.
    ///
    /// @spec docs/L0-search#smart-mailboxes
    fn query_messages_by_rule(
        &self,
        rule: &SmartMailboxRule,
    ) -> Result<Vec<MessageSummary>, StoreError>;

    /// Query messages matching a smart mailbox rule with seek pagination.
    ///
    /// @spec docs/L1-api#cursor-pagination
    fn query_message_page_by_rule(
        &self,
        rule: &SmartMailboxRule,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, StoreError>;

    /// Query conversations matching a smart mailbox rule with pagination.
    ///
    /// @spec docs/L1-sync#conversation-pagination
    fn query_conversations_by_rule(
        &self,
        rule: &SmartMailboxRule,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, StoreError>;

    /// Return (unread, total) counts for a smart mailbox rule.
    ///
    /// @spec docs/L1-search#smart-mailbox-data-model
    fn query_smart_mailbox_counts(&self, rule: &SmartMailboxRule)
        -> Result<(i64, i64), StoreError>;
}

/// Sync cursor state boundary.
pub trait SyncStateStore: Send + Sync {
    /// Load all stored sync cursors for an account.
    ///
    /// @spec docs/L1-sync#state-management
    fn get_sync_cursors(&self, account_id: &AccountId) -> Result<Vec<SyncCursor>, StoreError>;

    /// Load a single sync cursor by object type.
    ///
    /// @spec docs/L1-sync#state-management
    fn get_cursor(
        &self,
        account_id: &AccountId,
        object_type: SyncObject,
    ) -> Result<Option<SyncCursor>, StoreError>;
}

/// IMAP per-mailbox sync cursor read boundary.
///
/// @spec docs/L0-providers#imap-cursors-per-mailbox
pub trait ImapSyncStateStore: Send + Sync {
    fn list_imap_mailbox_states(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<ImapMailboxSyncState>, StoreError>;

    fn get_imap_mailbox_state(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
    ) -> Result<Option<ImapMailboxSyncState>, StoreError>;
}

/// IMAP per-mailbox sync cursor write boundary.
///
/// @spec docs/L0-providers#imap-cursors-per-mailbox
pub trait ImapSyncStateWriteStore: Send + Sync {
    fn put_imap_mailbox_state(
        &self,
        account_id: &AccountId,
        state: &ImapMailboxSyncState,
    ) -> Result<(), StoreError>;

    fn delete_imap_mailbox_state(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
    ) -> Result<(), StoreError>;
}

/// IMAP message location read boundary.
///
/// @spec docs/L0-providers#identity-and-threading
pub trait ImapMessageLocationStore: Send + Sync {
    fn list_imap_message_locations(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Vec<ImapMessageLocation>, StoreError>;

    fn list_imap_mailbox_message_locations(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
    ) -> Result<Vec<ImapMessageLocation>, StoreError>;
}

/// IMAP message location write boundary.
///
/// @spec docs/L0-providers#identity-and-threading
pub trait ImapMessageLocationWriteStore: Send + Sync {
    fn put_imap_message_location(
        &self,
        account_id: &AccountId,
        location: &ImapMessageLocation,
    ) -> Result<(), StoreError>;

    fn delete_imap_message_locations(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), StoreError>;
}

/// Message mailbox membership read boundary.
pub trait MessageMailboxStore: Send + Sync {
    /// Return current mailbox memberships for a message.
    ///
    /// @spec docs/L1-sync#sqlite-schema
    fn get_message_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Vec<MailboxId>, StoreError>;
}

/// Sync batch and lazy body write boundary.
pub trait SyncWriteStore: Send + Sync {
    /// Apply a sync batch atomically within a single SQLite transaction.
    ///
    /// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
    fn apply_sync_batch(
        &self,
        account_id: &AccountId,
        batch: &SyncBatch,
    ) -> Result<Vec<DomainEvent>, StoreError>;

    /// Persist a lazily-fetched message body.
    ///
    /// @spec docs/L1-sync#body-lazy
    fn apply_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        body: &FetchedBody,
    ) -> Result<CommandResult, StoreError>;
}

/// Local message mutation persistence boundary.
pub trait MessageCommandStore: Send + Sync {
    /// Apply a keyword mutation locally, updating the sync cursor.
    ///
    /// @spec docs/L1-jmap#methods-used
    fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        cursor: Option<&SyncCursor>,
        command: &SetKeywordsCommand,
    ) -> Result<CommandResult, StoreError>;

    /// Apply a mailbox replacement locally, updating the sync cursor.
    ///
    /// @spec docs/L1-jmap#methods-used
    fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        cursor: Option<&SyncCursor>,
        command: &ReplaceMailboxesCommand,
    ) -> Result<CommandResult, StoreError>;

    /// Delete a message locally, updating the sync cursor.
    ///
    /// @spec docs/L1-jmap#methods-used
    fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        cursor: Option<&SyncCursor>,
    ) -> Result<CommandResult, StoreError>;
}

/// Domain event log boundary.
pub trait EventStore: Send + Sync {
    /// Query the event log with optional filters.
    ///
    /// @spec docs/L1-api#sse-event-stream
    fn list_events(&self, filter: &EventFilter) -> Result<Vec<DomainEvent>, StoreError>;

    /// Append a domain event to the event log.
    ///
    /// @spec docs/L1-sync#event-propagation
    fn append_event(
        &self,
        account_id: &AccountId,
        topic: &str,
        mailbox_id: Option<&MailboxId>,
        message_id: Option<&MessageId>,
        payload: serde_json::Value,
    ) -> Result<DomainEvent, StoreError>;
}

/// Account/source projection maintenance boundary.
pub trait SourceProjectionStore: Send + Sync {
    /// Create or update the source projection row for sidebar display.
    ///
    /// @spec docs/L1-sync#sqlite-schema
    fn upsert_source_projection(&self, source_id: &AccountId, name: &str)
        -> Result<(), StoreError>;

    /// Remove the source projection row.
    ///
    /// @spec docs/L1-sync#sqlite-schema
    fn delete_source_projection(&self, source_id: &AccountId) -> Result<(), StoreError>;
}

/// Account-scoped synced data maintenance boundary.
pub trait SourceDataStore: Send + Sync {
    /// Delete all synced data for an account (messages, mailboxes, events).
    ///
    /// @spec docs/L0-accounts#the-invariant
    fn delete_source_data(&self, account_id: &AccountId) -> Result<(), StoreError>;
}

/// Durable cache of sender addresses that have already passed provider send
/// validation.
///
/// @spec docs/L1-compose#sender-selection
pub trait SenderAddressCacheStore: Send + Sync {
    /// List cached sender addresses across all configured account IDs.
    fn list_sender_address_cache(&self) -> Result<Vec<CachedSenderAddress>, StoreError>;

    /// Remember a sender address for the account that successfully submitted it.
    fn remember_sender_address(
        &self,
        account_id: &AccountId,
        sender: &Recipient,
    ) -> Result<(), StoreError>;
}

/// Durable automation backfill scheduling boundary.
pub trait AutomationBackfillStore: Send + Sync {
    /// Create the current account/rules job if it does not exist, returning the job.
    ///
    /// @spec docs/L1-sync#automation-actions
    fn ensure_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<AutomationBackfillJob, StoreError>;

    /// Mark a job as completed after all current matches have been processed.
    ///
    /// @spec docs/L1-sync#automation-actions
    fn complete_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<(), StoreError>;

    /// Record a worker failure while keeping the job pending for a later retry.
    ///
    /// @spec docs/L1-sync#automation-actions
    fn record_automation_backfill_failure(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
        error: &str,
    ) -> Result<(), StoreError>;

    /// Return the durable job for an account/rules fingerprint if one exists.
    ///
    /// @spec docs/L1-sync#automation-actions
    fn get_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<Option<AutomationBackfillJob>, StoreError>;
}

/// Local store for synced mail data, events, and projections.
///
/// The store is the single source of truth for the UI -- the frontend reads
/// via the REST API, never directly from JMAP.
///
/// @spec docs/L1-sync#sqlite-schema
pub trait MailStore:
    MailboxReadStore
    + MessageListStore
    + TagReadStore
    + ConversationReadStore
    + MessageDetailStore
    + SmartMailboxStore
    + SyncStateStore
    + ImapSyncStateStore
    + ImapMessageLocationStore
    + MessageMailboxStore
    + SyncWriteStore
    + MessageCommandStore
    + EventStore
    + SourceProjectionStore
    + SourceDataStore
    + SenderAddressCacheStore
    + AutomationBackfillStore
{
}

impl<T> MailStore for T where
    T: MailboxReadStore
        + MessageListStore
        + TagReadStore
        + ConversationReadStore
        + MessageDetailStore
        + SmartMailboxStore
        + SyncStateStore
        + ImapSyncStateStore
        + ImapMessageLocationStore
        + MessageMailboxStore
        + SyncWriteStore
        + MessageCommandStore
        + EventStore
        + SourceProjectionStore
        + SourceDataStore
        + SenderAddressCacheStore
        + AutomationBackfillStore
{
}

/// Credential storage abstraction (OS keyring or environment variables).
///
/// @spec docs/L0-accounts#credential-storage
/// @spec docs/L1-api#secret-management
pub trait SecretStore: Send + Sync {
    /// Resolve a secret reference to its plaintext value.
    fn resolve(&self, secret_ref: &SecretRef) -> Result<String, SecretStoreError>;
    /// Store a new secret.
    fn save(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError>;
    /// Replace an existing secret's value.
    fn update(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError>;
    /// Delete a stored secret.
    fn delete(&self, secret_ref: &SecretRef) -> Result<(), SecretStoreError>;
}

/// Thread-safe handle to a [`MailGateway`] implementation.
pub type SharedGateway = Arc<dyn MailGateway>;
/// Thread-safe handle to a [`SecretStore`] implementation.
pub type SharedSecretStore = Arc<dyn SecretStore>;

/// Extension trait for converting `Option<T>` into a not-found [`ServiceError`].
pub trait ServiceResultExt<T> {
    fn not_found(self, kind: &str, id: &str) -> Result<T, ServiceError>;
}

impl<T> ServiceResultExt<T> for Option<T> {
    fn not_found(self, kind: &str, id: &str) -> Result<T, ServiceError> {
        self.ok_or_else(|| StoreError::NotFound(format!("{kind}:{id}")).into())
    }
}
