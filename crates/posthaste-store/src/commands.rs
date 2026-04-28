use super::*;

impl SyncWriteStore for DatabaseStore {
    /// Applies a sync batch within a single SQLite transaction: stages raw
    /// bodies to disk first, then upserts/deletes mailboxes and messages,
    /// refreshes projections, and persists cursors atomically with data.
    ///
    /// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
    fn apply_sync_batch(
        &self,
        account_id: &AccountId,
        batch: &SyncBatch,
    ) -> Result<Vec<DomainEvent>, StoreError> {
        debug!(
            account_id = %account_id,
            mailboxes = batch.mailboxes.len(),
            messages = batch.messages.len(),
            "applying sync batch to store"
        );
        let started = Instant::now();
        let staged_bodies = stage_sync_bodies(self, account_id, batch)?;
        let events = self
            .write_transaction(|tx| apply_sync_batch_tx(tx, account_id, batch, &staged_bodies))?;
        info!(
            account_id = %account_id,
            mailbox_count = batch.mailboxes.len(),
            message_count = batch.messages.len(),
            deleted_mailbox_count = batch.deleted_mailbox_ids.len(),
            deleted_message_count = batch.deleted_message_ids.len(),
            event_count = events.len(),
            duration_ms = started.elapsed().as_millis() as u64,
            "sync batch applied to store"
        );
        Ok(events)
    }

    /// Stores a lazily fetched message body and emits
    /// `EVENT_TOPIC_MESSAGE_BODY_CACHED`.
    ///
    /// @spec docs/L1-sync#invariants
    fn apply_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        body: &FetchedBody,
    ) -> Result<CommandResult, StoreError> {
        let raw_ref = body
            .raw_mime
            .as_deref()
            .map(|raw_mime| self.store_raw_message(account_id, raw_mime))
            .transpose()?;
        self.write_transaction(|tx| {
            apply_message_body_tx(tx, account_id, message_id, body, raw_ref.as_ref())
        })
    }
}

impl MessageCommandStore for DatabaseStore {
    /// Adds/removes keywords on a message and refreshes mailbox counters.
    /// Optionally persists a new sync cursor atomically.
    fn set_keywords(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        cursor: Option<&SyncCursor>,
        command: &SetKeywordsCommand,
    ) -> Result<CommandResult, StoreError> {
        self.write_transaction(|tx| set_keywords_tx(tx, account_id, message_id, cursor, command))
    }

    /// Replaces a message's mailbox memberships, refreshes counters, and emits
    /// arrival events for newly added mailboxes. Optionally persists a cursor.
    fn replace_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        cursor: Option<&SyncCursor>,
        command: &ReplaceMailboxesCommand,
    ) -> Result<CommandResult, StoreError> {
        self.write_transaction(|tx| {
            replace_mailboxes_tx(tx, account_id, message_id, cursor, command)
        })
    }

    /// Permanently deletes a message and all its junction rows, refreshes
    /// thread/mailbox projections, and optionally persists a cursor.
    fn destroy_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        cursor: Option<&SyncCursor>,
    ) -> Result<CommandResult, StoreError> {
        self.write_transaction(|tx| destroy_message_tx(tx, account_id, message_id, cursor))
    }
}

impl EventStore for DatabaseStore {
    /// Queries the event log, supporting `afterSeq` cursor-based replay.
    ///
    /// @spec docs/L1-sync#event-propagation
    fn list_events(&self, filter: &EventFilter) -> Result<Vec<DomainEvent>, StoreError> {
        let connection = self.read_connection()?;
        list_events_for_filter(&connection, filter)
    }

    /// Inserts a domain event into the event log.
    ///
    /// @spec docs/L1-sync#event-propagation
    fn append_event(
        &self,
        account_id: &AccountId,
        topic: &str,
        mailbox_id: Option<&MailboxId>,
        message_id: Option<&MessageId>,
        payload: Value,
    ) -> Result<DomainEvent, StoreError> {
        self.write_transaction(|tx| {
            insert_event_tx(tx, account_id, topic, mailbox_id, message_id, payload)
        })
    }
}
