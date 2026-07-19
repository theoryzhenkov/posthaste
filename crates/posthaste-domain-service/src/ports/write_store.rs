use super::*;

// NS1 → NS2 Slice 3: `MessageCommandStore` is GONE. State assertions fold
// into the overlay plane (NS1), and the last non-reconciler base writer — the
// draft-discard optimistic destroy — died when discards became tombstone
// folds. Sync's reconciler is the only production base writer left.

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
    /// order. Two readiness gates on two clocks (D152): `wall_now` (normalized
    /// RFC 3339, RE-SAMPLED wall time) gates wall-scheduled sends (`send_at`,
    /// send-later); `mono_now` (the daemon's monotonic-anchored epoch seconds)
    /// gates undo holds (`hold_until_mono`) — the same clock that STAMPED
    /// them. A held op rests `pending` until due, so a not-yet-due send is
    /// never pushed.
    fn list_flushable_operations(
        &self,
        account_id: &AccountId,
        wall_now: &str,
        mono_now: i64,
    ) -> Result<Vec<Operation>, StoreError>;

    /// Number of held sends now due on EITHER clock (see
    /// [`Self::list_flushable_operations`]) and still queued. The scheduler
    /// tick's probe: a non-zero count triggers a flush sync so a due send
    /// fires promptly instead of waiting for the next poll window.
    fn count_due_scheduled_sends(
        &self,
        account_id: &AccountId,
        wall_now: &str,
        mono_now: i64,
    ) -> Result<u64, StoreError>;

    /// Operations to surface as outstanding work in the outbox **UI**:
    /// everything except `applied`. An `applied` op is provider-accepted and
    /// awaiting silent convergence, so it is not user-facing work. This is NOT
    /// the overlay source — use [`Self::list_unsettled_operations`] to fold
    /// reads. In insertion order.
    fn list_pending_operations(&self, account_id: &AccountId)
        -> Result<Vec<Operation>, StoreError>;

    /// The read-time **overlay** source: every operation whose optimistic
    /// effect may still need folding — `pending`, `inflight`, and settled
    /// (`applied`) ops awaiting causal truncation (a flushed message assertion
    /// the provider accepted; its effect keeps serving until the sync chain
    /// absorbs it). Excludes only `failed`. In insertion order.
    ///
    /// @spec docs/backend/L2-optimism#settlement-and-truncation
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

    /// Atomically replace a still-`pending` operation's payload (D174 draft-save
    /// coalescing: last-writer-wins per compose session), returning whether the
    /// swap landed. `false` means the flusher claimed the op concurrently — the
    /// caller enqueues a fresh operation instead. Resets `attempts`/`last_error`
    /// (the new payload is a new piece of work). Never changes id or kind.
    fn replace_operation_payload(
        &self,
        id: &OperationId,
        payload: &serde_json::Value,
    ) -> Result<bool, StoreError>;

    /// Rewrite a temporary entity id to its reconciled provider id across all of
    /// an account's operations (temp-id reconciliation after first flush).
    fn reconcile_operation_entity_id(
        &self,
        account_id: &AccountId,
        from_entity_id: &str,
        to_entity_id: &str,
    ) -> Result<(), StoreError>;

    /// Settle an operation IN PLACE: state becomes `applied` and the causal
    /// truncation markers are recorded — `settled_at_mono` (the daemon's
    /// monotonic-anchored epoch seconds at settlement) and, when the provider
    /// named a sync position that includes the change, `watermark` (the
    /// stored-cursor encoding of that position). The op stays in the log:
    /// excluded from the flush lane and from pendingOperations, still folded
    /// by replay, until truncation removes it.
    fn mark_operation_settled(
        &self,
        id: &OperationId,
        settled_at_mono: i64,
        watermark: Option<&str>,
    ) -> Result<(), StoreError>;

    /// The settled (`applied`) operations for an account with their causal
    /// truncation markers, in insertion order — the truncation pass's read.
    fn list_settled_operations(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<SettledOperation>, StoreError>;

    /// Remove an operation from the log: a settlement whose base write made
    /// causality hold by construction (readback/rejection), a truncated
    /// settled op, or a discarded one.
    fn remove_operation(&self, id: &OperationId) -> Result<(), StoreError>;

    /// Atomically transition a still-flushable operation to `inflight`,
    /// returning whether the claim won. The flusher's entry gate for every
    /// push: the state predicate and the write are one statement, so a
    /// concurrent [`Self::remove_operation_unless_inflight`] (user cancel)
    /// and this claim have exactly one winner — a discarded op is never
    /// pushed, and a claimed op can no longer be discarded.
    fn claim_operation_for_flush(&self, id: &OperationId) -> Result<bool, StoreError>;

    /// Atomically remove an operation unless it is `inflight` or `applied`,
    /// returning whether a row was removed. The user-cancel half of the
    /// cancel-vs-flush race — see [`Self::claim_operation_for_flush`]. A
    /// settled (`applied`) op is likewise untouchable: it rests in the log
    /// until causal truncation, so a late cancel observes it as already gone.
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
