use super::*;

impl MailboxRoleOverrideStore for DatabaseStore {
    /// Stores a local mailbox role override for providers whose mailbox role
    /// metadata cannot be changed remotely.
    fn set_mailbox_role_override(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
        role: Option<&str>,
        clear_role_from: Option<&MailboxId>,
    ) -> Result<(), StoreError> {
        validate_mailbox_role(role)?;
        let updated_at = now_iso8601()?;
        self.write_transaction(|tx| {
            if let Some(clear_role_from) = clear_role_from.filter(|id| *id != mailbox_id) {
                upsert_mailbox_role_override_tx(
                    tx,
                    account_id,
                    clear_role_from,
                    None,
                    &updated_at,
                )?;
                update_mailbox_role_tx(tx, account_id, clear_role_from, None)?;
            }
            ensure_mailbox_role_is_available_tx(tx, account_id, mailbox_id, role)?;
            upsert_mailbox_role_override_tx(tx, account_id, mailbox_id, role, &updated_at)?;
            update_mailbox_role_tx(tx, account_id, mailbox_id, role)
        })
    }
}

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
        ph_debug!(
            events::STORE_SYNC_BATCH_APPLYING,
            account_id = %account_id,
            mailboxes = batch.mailboxes.len(),
            messages = batch.messages.len(),
            "applying sync batch to store"
        );
        let started = Instant::now();
        let events = self.staged_write(
            |store, staged| stage_sync_bodies(store, account_id, batch, staged),
            |tx, staged_bodies| apply_sync_batch_tx(tx, account_id, batch, staged_bodies),
        )?;
        ph_info!(
            events::STORE_SYNC_BATCH_APPLIED,
            account_id = %account_id,
            mailbox_count = batch.mailboxes.len(),
            message_count = batch.messages.len(),
            deleted_mailbox_count = batch.deleted_mailbox_ids.len(),
            deleted_imap_location_count = batch.deleted_imap_message_locations.len(),
            deleted_message_count = batch.deleted_message_ids.len(),
            event_count = events.len(),
            duration_ms = started.elapsed().as_millis() as u64,
            "sync batch applied to store"
        );
        Ok(events)
    }

    /// Runs the streamed final reconciliation pass within a single SQLite
    /// transaction: prunes locals absent from the complete remote id set and
    /// commits the cursors withheld until the full stream succeeded.
    ///
    fn reconcile_sync(
        &self,
        account_id: &AccountId,
        reconciliation: &SyncReconciliation,
    ) -> Result<Vec<DomainEvent>, StoreError> {
        ph_debug!(
            events::STORE_SYNC_BATCH_APPLYING,
            account_id = %account_id,
            prune_mailboxes = reconciliation.prune_mailboxes,
            prune_messages = reconciliation.prune_messages,
            remote_mailbox_count = reconciliation.remote_mailbox_ids.len(),
            remote_message_count = reconciliation.remote_message_ids.len(),
            "reconciling streamed sync"
        );
        let started = Instant::now();
        let events =
            self.write_transaction(|tx| reconcile_sync_tx(tx, account_id, reconciliation))?;
        ph_info!(
            events::STORE_SYNC_BATCH_APPLIED,
            account_id = %account_id,
            event_count = events.len(),
            duration_ms = started.elapsed().as_millis() as u64,
            "streamed sync reconciled"
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
        self.staged_write(
            |store, staged| {
                body.raw_mime
                    .as_deref()
                    .map(|raw_mime| store.store_raw_message(account_id, raw_mime, staged))
                    .transpose()
            },
            |tx, raw_ref| apply_message_body_tx(tx, account_id, message_id, body, raw_ref.as_ref()),
        )
    }
}

fn upsert_mailbox_role_override_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    mailbox_id: &MailboxId,
    role: Option<&str>,
    updated_at: &str,
) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO mailbox_role_override (account_id, mailbox_id, role, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(account_id, mailbox_id) DO UPDATE SET
            role = excluded.role,
            updated_at = excluded.updated_at",
        params![account_id.as_str(), mailbox_id.as_str(), role, updated_at,],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

fn validate_mailbox_role(role: Option<&str>) -> Result<(), StoreError> {
    if let Some(role) = role.filter(|role| MailboxRole::parse(role).is_none()) {
        return Err(StoreError::Conflict(format!(
            "unsupported mailbox role: {role}"
        )));
    }
    Ok(())
}

fn ensure_mailbox_role_is_available_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    mailbox_id: &MailboxId,
    role: Option<&str>,
) -> Result<(), StoreError> {
    let Some(role) = role else {
        return Ok(());
    };
    let owner = tx
        .query_row(
            "SELECT id FROM mailbox
             WHERE account_id = ?1 AND role = ?2 AND id != ?3
             LIMIT 1",
            params![account_id.as_str(), role, mailbox_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_to_store_error)?;
    if let Some(owner) = owner {
        return Err(StoreError::Conflict(format!(
            "mailbox role {role} already assigned to mailbox:{owner}"
        )));
    }
    Ok(())
}

fn update_mailbox_role_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    mailbox_id: &MailboxId,
    role: Option<&str>,
) -> Result<(), StoreError> {
    let affected = tx
        .execute(
            "UPDATE mailbox
             SET role = ?3
             WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), mailbox_id.as_str(), role],
        )
        .map_err(sql_to_store_error)?;
    if affected == 0 {
        return Err(StoreError::NotFound(format!("mailbox:{mailbox_id}")));
    }
    Ok(())
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

    /// The cheap `(MIN(seq), MAX(seq))` head/oldest query for the fact-carrying
    /// tap (RFC-L2-scripting S2), replacing the full replay scan.
    fn event_log_bounds(&self) -> Result<Option<EventLogBounds>, StoreError> {
        let connection = self.read_connection()?;
        event_log_bounds_query(&connection)
    }
}
