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
