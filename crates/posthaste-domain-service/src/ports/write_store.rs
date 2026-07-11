use super::*;

/// Local message mutation persistence boundary.
///
/// Stays plain synchronous `&self` (not `async`, D63/M23b design note): like
/// [`SyncWriteStore`](super::SyncWriteStore), `posthaste-store`'s own unit
/// tests call these directly on a bare `DatabaseStore` with no runtime — an
/// `async` port would force every one of those tests to acquire one. The
/// async offload lives at the call site instead: `MailService` reaches these
/// through `Arc<dyn MessageCommandStore>` and, from its async methods, wraps
/// the call in `tokio::task::spawn_blocking` (`apply_assertion_to_canonical`)
/// so the per-message-action write — invoked directly from the HTTP request
/// path — never occupies a tokio worker thread.
///
/// @spec docs/eph/RFC-L2-lifecycle-and-errors#d63
/// NS1: `set_keywords`/`replace_mailboxes` are GONE — state assertions fold
/// into the overlay plane and never write base. One method survives for the
/// last non-reconciler base writer (the draft-discard destroy), sealed behind
/// the [`BaseWrite`] witness until its NS2 cutover deletes it too.
pub trait MessageCommandStore: Send + Sync {
    /// Delete a message row from BASE, updating the sync cursor.
    ///
    /// @spec docs/L1-jmap#methods-used
    fn destroy_message(
        &self,
        base: &BaseWrite,
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

    /// The cheap seq bounds of `event_log` — `(MIN(seq), MAX(seq))` — for the
    /// fact-carrying tap's head/truncation queries, without scanning the log
    /// (RFC-L2-scripting D52 / S2). `Ok(None)` when the log is empty. The default
    /// returns `Ok(None)` so a store that does not implement it degrades to the
    /// tap's replay-scan fallback rather than failing.
    ///
    /// @spec docs/eph/RFC-L2-scripting#4-d52-the-tap
    fn event_log_bounds(&self) -> Result<Option<EventLogBounds>, StoreError> {
        Ok(None)
    }
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
    /// order. `now` (normalized UTC whole-second RFC 3339) gates scheduled
    /// sends: an op whose `send_at` is after `now` is HELD out of the result —
    /// it rests `pending` until due, so a not-yet-due send is never pushed.
    fn list_flushable_operations(
        &self,
        account_id: &AccountId,
        now: &str,
    ) -> Result<Vec<Operation>, StoreError>;

    /// Number of scheduled sends (`send_at` set) that are due (`send_at <=
    /// now`) and still queued. The scheduler tick's probe: a non-zero count
    /// triggers a flush sync so a due send fires promptly instead of waiting
    /// for the next poll window.
    fn count_due_scheduled_sends(
        &self,
        account_id: &AccountId,
        now: &str,
    ) -> Result<u64, StoreError>;

    /// Operations to surface as outstanding work in the outbox **UI**:
    /// everything except `applied`. An `applied` op is provider-accepted and
    /// awaiting silent convergence, so it is not user-facing work. This is NOT
    /// the overlay source — use [`Self::list_unsettled_operations`] to fold
    /// reads. In insertion order.
    fn list_pending_operations(&self, account_id: &AccountId)
        -> Result<Vec<Operation>, StoreError>;

    /// The read-time **overlay** source: every operation whose optimistic
    /// effect may still need folding — `pending`, `inflight`, and
    /// `applied`-awaiting-convergence (a flushed message assertion the provider
    /// accepted but a sync has not yet confirmed into the projection). Excludes
    /// only `failed`. In insertion order.
    ///
    /// @spec docs/replication/L1#retire-on-confirmation
    fn list_unsettled_operations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<Operation>, StoreError>;

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

    /// Atomically transition a still-flushable operation to `inflight`,
    /// returning whether the claim won. The flusher's entry gate for every
    /// push: the state predicate and the write are one statement, so a
    /// concurrent [`Self::remove_operation_unless_inflight`] (user cancel)
    /// and this claim have exactly one winner — a discarded op is never
    /// pushed, and a claimed op can no longer be discarded.
    fn claim_operation_for_flush(&self, id: &OperationId) -> Result<bool, StoreError>;

    /// Atomically remove an operation unless it is `inflight`, returning
    /// whether a row was removed. The user-cancel half of the cancel-vs-flush
    /// race — see [`Self::claim_operation_for_flush`].
    fn remove_operation_unless_inflight(&self, id: &OperationId) -> Result<bool, StoreError>;
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

    /// Force a job back to pending (creating it if absent), so an on-demand
    /// backfill re-runs even when the same rules previously completed.
    ///
    /// @spec docs/L1-sync#automation-actions
    fn reset_automation_backfill_job(
        &self,
        account_id: &AccountId,
        rule_fingerprint: &str,
    ) -> Result<AutomationBackfillJob, StoreError>;
}
