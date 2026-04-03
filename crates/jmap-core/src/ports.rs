use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    AccountId, CommandResult, ConversationCursor, ConversationId, ConversationPage,
    ConversationView, EventFilter, FetchedBody, Identity, MailboxId, MailboxSummary, MessageDetail,
    MessageId, MessageSummary, MutationOutcome, PushTransport, ReplaceMailboxesCommand,
    ReplyContext, SecretRef, SecretStoreError, SendMessageRequest, SetKeywordsCommand,
    SmartMailboxRule, SyncBatch, SyncCursor, SyncObject, ThreadId, ThreadView,
};
use crate::{DomainEvent, GatewayError, ServiceError, StoreError};

/// Gateway to a remote JMAP server.
///
/// Abstracts JMAP protocol operations behind a domain-level interface.
/// Implementations: `LiveJmapGateway` for real JMAP, `MockGateway` for tests.
///
/// @spec spec/L1-jmap#methods-used
#[async_trait]
pub trait MailGateway: Send + Sync {
    /// Perform a delta or full sync for all object types using stored cursors.
    ///
    /// @spec spec/L1-sync#sync-loop
    async fn sync(
        &self,
        account_id: &AccountId,
        cursors: &[SyncCursor],
    ) -> Result<SyncBatch, GatewayError>;

    /// Lazily fetch body content for a single message.
    ///
    /// @spec spec/L1-sync#sync-loop
    async fn fetch_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<FetchedBody, GatewayError>;

    /// Update JMAP keywords on a message via `Email/set`.
    ///
    /// @spec spec/L1-jmap#methods-used
    async fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        command: &SetKeywordsCommand,
    ) -> Result<MutationOutcome, GatewayError>;

    /// Atomically replace all mailbox memberships for a message via `Email/set`.
    ///
    /// @spec spec/L1-jmap#methods-used
    async fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
        mailbox_ids: &[MailboxId],
    ) -> Result<MutationOutcome, GatewayError>;

    /// Permanently delete a message via `Email/set`.
    ///
    /// @spec spec/L1-jmap#methods-used
    async fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        expected_state: Option<&str>,
    ) -> Result<MutationOutcome, GatewayError>;

    /// Fetch the primary sender identity via `Identity/get`.
    ///
    /// @spec spec/L1-jmap#methods-used
    async fn fetch_identity(&self, account_id: &AccountId) -> Result<Identity, GatewayError>;

    /// Fetch reply/forward metadata for composing a response.
    ///
    /// @spec spec/L1-jmap#methods-used
    async fn fetch_reply_context(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<ReplyContext, GatewayError>;

    /// Send an email via `EmailSubmission/set`.
    ///
    /// @spec spec/L1-jmap#methods-used
    async fn send_message(
        &self,
        account_id: &AccountId,
        request: &SendMessageRequest,
    ) -> Result<(), GatewayError>;

    /// Return available push transports ordered by preference (WS first, then SSE).
    ///
    /// @spec spec/L2-transport#new-abstractions
    fn push_transports(&self) -> Vec<Box<dyn PushTransport>>;
}

/// Local SQLite store for synced mail data, events, and projections.
///
/// The store is the single source of truth for the UI -- the frontend reads
/// via the REST API, never directly from JMAP.
///
/// @spec spec/L1-sync#sqlite-schema
pub trait MailStore: Send + Sync {
    /// List all mailboxes for an account.
    ///
    /// @spec spec/L1-sync#sqlite-schema
    fn list_mailboxes(&self, account_id: &AccountId) -> Result<Vec<MailboxSummary>, StoreError>;

    /// List messages, optionally filtered by mailbox.
    ///
    /// @spec spec/L1-sync#sqlite-schema
    fn list_messages(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, StoreError>;

    /// Query messages matching a smart mailbox rule.
    ///
    /// @spec spec/L0-search#smart-mailboxes
    fn query_messages_by_rule(
        &self,
        rule: &SmartMailboxRule,
    ) -> Result<Vec<MessageSummary>, StoreError>;

    /// Query conversations matching a smart mailbox rule with pagination.
    ///
    /// @spec spec/L1-sync#conversation-pagination
    fn query_conversations_by_rule(
        &self,
        rule: &SmartMailboxRule,
        limit: usize,
        cursor: Option<&ConversationCursor>,
    ) -> Result<ConversationPage, StoreError>;

    /// Return (unread, total) counts for a smart mailbox rule.
    ///
    /// @spec spec/L1-search#smart-mailbox-data-model
    fn query_smart_mailbox_counts(&self, rule: &SmartMailboxRule)
        -> Result<(i64, i64), StoreError>;

    /// Paginated conversation list with seek-based cursors.
    ///
    /// @spec spec/L1-sync#conversation-pagination
    fn list_conversations(
        &self,
        account_id: Option<&AccountId>,
        mailbox_id: Option<&MailboxId>,
        limit: usize,
        cursor: Option<&ConversationCursor>,
    ) -> Result<ConversationPage, StoreError>;

    /// Fetch a single conversation with all its messages.
    ///
    /// @spec spec/L1-sync#conversation-pagination
    fn get_conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<Option<ConversationView>, StoreError>;

    /// Fetch full message detail including body content.
    ///
    /// @spec spec/L1-sync#body-lazy
    fn get_message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, StoreError>;

    /// Fetch all messages in a thread.
    ///
    /// @spec spec/L1-sync#sqlite-schema
    fn get_thread(
        &self,
        account_id: &AccountId,
        thread_id: &ThreadId,
    ) -> Result<Option<ThreadView>, StoreError>;

    /// Load all stored sync cursors for an account.
    ///
    /// @spec spec/L1-sync#state-management
    fn get_sync_cursors(&self, account_id: &AccountId) -> Result<Vec<SyncCursor>, StoreError>;

    /// Load a single sync cursor by object type.
    ///
    /// @spec spec/L1-sync#state-management
    fn get_cursor(
        &self,
        account_id: &AccountId,
        object_type: SyncObject,
    ) -> Result<Option<SyncCursor>, StoreError>;

    /// Return current mailbox memberships for a message.
    ///
    /// @spec spec/L1-sync#sqlite-schema
    fn get_message_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Vec<MailboxId>, StoreError>;

    /// Apply a sync batch atomically within a single SQLite transaction.
    ///
    /// @spec spec/L1-sync#syncbatch-and-apply_sync_batch
    fn apply_sync_batch(
        &self,
        account_id: &AccountId,
        batch: &SyncBatch,
    ) -> Result<Vec<DomainEvent>, StoreError>;

    /// Persist a lazily-fetched message body.
    ///
    /// @spec spec/L1-sync#body-lazy
    fn apply_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        body: &FetchedBody,
    ) -> Result<CommandResult, StoreError>;

    /// Apply a keyword mutation locally, updating the sync cursor.
    ///
    /// @spec spec/L1-jmap#methods-used
    fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        cursor: Option<&SyncCursor>,
        command: &SetKeywordsCommand,
    ) -> Result<CommandResult, StoreError>;

    /// Apply a mailbox replacement locally, updating the sync cursor.
    ///
    /// @spec spec/L1-jmap#methods-used
    fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        cursor: Option<&SyncCursor>,
        command: &ReplaceMailboxesCommand,
    ) -> Result<CommandResult, StoreError>;

    /// Delete a message locally, updating the sync cursor.
    ///
    /// @spec spec/L1-jmap#methods-used
    fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        cursor: Option<&SyncCursor>,
    ) -> Result<CommandResult, StoreError>;

    /// Query the event log with optional filters.
    ///
    /// @spec spec/L1-api#sse-event-stream
    fn list_events(&self, filter: &EventFilter) -> Result<Vec<DomainEvent>, StoreError>;

    /// Append a domain event to the event log.
    ///
    /// @spec spec/L1-sync#event-propagation
    fn append_event(
        &self,
        account_id: &AccountId,
        topic: &str,
        mailbox_id: Option<&MailboxId>,
        message_id: Option<&MessageId>,
        payload: serde_json::Value,
    ) -> Result<DomainEvent, StoreError>;

    /// Create or update the source projection row for sidebar display.
    ///
    /// @spec spec/L1-sync#sqlite-schema
    fn upsert_source_projection(&self, source_id: &AccountId, name: &str)
        -> Result<(), StoreError>;

    /// Remove the source projection row.
    ///
    /// @spec spec/L1-sync#sqlite-schema
    fn delete_source_projection(&self, source_id: &AccountId) -> Result<(), StoreError>;

    /// Delete all synced data for an account (messages, mailboxes, events).
    ///
    /// @spec spec/L0-accounts#the-invariant
    fn delete_source_data(&self, account_id: &AccountId) -> Result<(), StoreError>;
}

/// Credential storage abstraction (OS keyring or environment variables).
///
/// @spec spec/L0-accounts#credential-storage
/// @spec spec/L1-api#secret-management
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

/// Thread-safe handle to a [`MailStore`] implementation.
pub type SharedStore = Arc<dyn MailStore>;
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
