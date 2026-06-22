use super::*;

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

/// Durable local-first command outbox boundary (Tier 2: runtime <-> provider).
///
/// Operations are reflected through a read-time overlay and flushed to the
/// provider later; this is their durable queue. `id` is the runtime/provider
/// idempotency key, so enqueue is idempotent and a settled id is never pushed
/// twice.
///
/// @spec docs/L1-outbox#operation-model
pub trait OperationOutboxStore: Send + Sync {
    /// Persist a new operation. Idempotent on [`Operation::id`]: re-enqueuing an
    /// existing id returns the already-stored operation unchanged.
    fn enqueue_operation(&self, operation: &Operation) -> Result<Operation, StoreError>;

    /// Operations eligible for flushing (pending) for an account, in insertion
    /// order.
    fn list_flushable_operations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<Operation>, StoreError>;

    /// All non-terminal operations for an account, in insertion order. Used to
    /// hydrate optimistic state and surface pending/failed work to the UI.
    fn list_pending_operations(&self, account_id: &AccountId)
        -> Result<Vec<Operation>, StoreError>;

    /// Fetch a single operation by id.
    fn get_operation(&self, id: &OperationId) -> Result<Option<Operation>, StoreError>;

    /// Update an operation's lifecycle state, bumping `updated_at` and recording
    /// `attempts` / `last_error`.
    fn update_operation_state(
        &self,
        id: &OperationId,
        state: OperationState,
        attempts: u32,
        last_error: Option<&str>,
    ) -> Result<(), StoreError>;

    /// Rewrite a temporary entity id to its reconciled provider id across all of
    /// an account's operations (temp-id reconciliation after first flush).
    fn reconcile_operation_entity_id(
        &self,
        account_id: &AccountId,
        from_entity_id: &str,
        to_entity_id: &str,
    ) -> Result<(), StoreError>;

    /// Remove an operation after it has settled and been propagated downstream.
    fn remove_operation(&self, id: &OperationId) -> Result<(), StoreError>;

    /// Resolve a stable client draft key to the entity id currently representing
    /// that draft (a temporary id before its first flush, a provider id after).
    /// Returns `None` for a key never saved before.
    ///
    /// @spec docs/L1-outbox#temp-id-reconciliation
    fn resolve_draft_entity(
        &self,
        account_id: &AccountId,
        draft_key: &str,
    ) -> Result<Option<String>, StoreError>;

    /// Record the entity id a client draft key currently maps to.
    fn set_draft_alias(
        &self,
        account_id: &AccountId,
        draft_key: &str,
        entity_id: &str,
    ) -> Result<(), StoreError>;

    /// Rewrite draft-alias entity ids from a temporary/old id to a newly assigned
    /// provider id, keeping the stable client key pointed at the live draft.
    ///
    /// @spec docs/L1-outbox#temp-id-reconciliation
    fn update_draft_alias_entity(
        &self,
        account_id: &AccountId,
        from_entity_id: &str,
        to_entity_id: &str,
    ) -> Result<(), StoreError>;

    /// Drop a client draft key's alias (after the draft is deleted).
    fn remove_draft_alias(&self, account_id: &AccountId, draft_key: &str)
        -> Result<(), StoreError>;
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
