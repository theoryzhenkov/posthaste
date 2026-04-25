use super::*;
use crate::projections::{
    assign_conversation_id_tx, delete_message_tx, normalized_subject,
    refresh_conversation_projection_tx, refresh_mailbox_counters_tx, refresh_thread_projection_tx,
    replace_attachments_tx, upsert_body_tx,
};
use crate::query::{
    fetch_keywords_tx, fetch_mailbox_ids_tx, query_message_detail_tx, row_to_event,
};

/// Stages raw MIME bodies to disk before the write transaction so that file
/// I/O does not block the SQLite lock. Falls back to synthesizing a minimal
/// RFC 822 message when `raw_mime` is absent but body HTML/text is present.
pub(crate) fn stage_sync_bodies(
    store: &DatabaseStore,
    account_id: &AccountId,
    batch: &SyncBatch,
) -> Result<Vec<Option<RawMessageRef>>, StoreError> {
    batch
        .messages
        .iter()
        .map(|message| {
            let raw_mime = message
                .raw_mime
                .clone()
                .or_else(|| synthesize_raw_mime(message));
            raw_mime
                .as_deref()
                .map(|raw_mime| store.store_raw_message(account_id, raw_mime))
                .transpose()
        })
        .collect()
}

/// Core sync write path: applies a `SyncBatch` within one SQLite transaction.
/// Handles mailbox snapshot replacement, deletes, upserts, keyword/mailbox
/// junction updates, conversation assignment, projection refreshes, and cursor
/// persistence. Emits domain events for each mutation.
///
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
pub(crate) fn apply_sync_batch_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    batch: &SyncBatch,
    staged_bodies: &[Option<RawMessageRef>],
) -> Result<Vec<DomainEvent>, StoreError> {
    let mut events = Vec::new();
    let mut affected_mailboxes = BTreeSet::new();
    let mut affected_threads = BTreeSet::new();
    let mut affected_conversations = BTreeSet::new();

    if batch.replace_all_mailboxes {
        let remote_mailbox_ids: BTreeSet<_> = batch
            .mailboxes
            .iter()
            .map(|mailbox| mailbox.id.clone())
            .collect();
        let mut statement = tx
            .prepare("SELECT id FROM mailbox WHERE account_id = ?1")
            .map_err(sql_to_store_error)?;
        let local_mailbox_ids = statement
            .query_map(params![account_id.as_str()], |row| row.get::<_, String>(0))
            .map_err(sql_to_store_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)?
            .into_iter()
            .map(MailboxId)
            .collect::<BTreeSet<_>>();

        for mailbox_id in local_mailbox_ids.difference(&remote_mailbox_ids) {
            tx.execute(
                "DELETE FROM mailbox WHERE account_id = ?1 AND id = ?2",
                params![account_id.as_str(), mailbox_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            affected_mailboxes.insert(mailbox_id.clone());
            events.push(insert_event_tx(
                tx,
                account_id,
                EVENT_TOPIC_MAILBOX_UPDATED,
                Some(mailbox_id),
                None,
                json!({ "mailboxId": mailbox_id.as_str(), "deleted": true }),
            )?);
        }
    }

    if batch.replace_all_messages {
        let remote_message_ids: BTreeSet<_> = batch
            .messages
            .iter()
            .map(|message| message.id.clone())
            .collect();
        let mut statement = tx
            .prepare("SELECT id FROM message WHERE account_id = ?1")
            .map_err(sql_to_store_error)?;
        let local_message_ids = statement
            .query_map(params![account_id.as_str()], |row| row.get::<_, String>(0))
            .map_err(sql_to_store_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)?
            .into_iter()
            .map(MessageId)
            .collect::<BTreeSet<_>>();

        for message_id in local_message_ids.difference(&remote_message_ids) {
            delete_message_and_track_projection_inputs(
                tx,
                account_id,
                message_id,
                &mut affected_mailboxes,
                &mut affected_threads,
                &mut affected_conversations,
            )?;
            events.push(insert_event_tx(
                tx,
                account_id,
                EVENT_TOPIC_MESSAGE_UPDATED,
                None,
                Some(message_id),
                json!({ "messageId": message_id.as_str(), "deleted": true }),
            )?);
        }
    }

    for mailbox_id in &batch.deleted_mailbox_ids {
        tx.execute(
            "DELETE FROM mailbox WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), mailbox_id.as_str()],
        )
        .map_err(sql_to_store_error)?;
        affected_mailboxes.insert(mailbox_id.clone());
        events.push(insert_event_tx(
            tx,
            account_id,
            EVENT_TOPIC_MAILBOX_UPDATED,
            Some(mailbox_id),
            None,
            json!({ "mailboxId": mailbox_id.as_str(), "deleted": true }),
        )?);
    }

    for message_id in &batch.deleted_message_ids {
        delete_message_and_track_projection_inputs(
            tx,
            account_id,
            message_id,
            &mut affected_mailboxes,
            &mut affected_threads,
            &mut affected_conversations,
        )?;
    }

    for mailbox in &batch.mailboxes {
        tx.execute(
            "INSERT INTO mailbox (account_id, id, name, role, unread_emails, total_emails)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(account_id, id) DO UPDATE SET
                name = excluded.name,
                role = excluded.role,
                unread_emails = excluded.unread_emails,
                total_emails = excluded.total_emails",
            params![
                account_id.as_str(),
                mailbox.id.as_str(),
                mailbox.name,
                mailbox.role,
                mailbox.unread_emails,
                mailbox.total_emails
            ],
        )
        .map_err(sql_to_store_error)?;
        events.push(insert_event_tx(
            tx,
            account_id,
            EVENT_TOPIC_MAILBOX_UPDATED,
            Some(&mailbox.id),
            None,
            json!({ "mailboxId": mailbox.id.as_str() }),
        )?);
    }

    for (message, raw_ref) in batch.messages.iter().zip(staged_bodies.iter()) {
        let previous_mailboxes = fetch_mailbox_ids_tx(tx, account_id, &message.id)?;
        let previous_keywords = fetch_keywords_tx(tx, account_id, &message.id)?;
        let previous_conversation_id = tx
            .query_row(
                "SELECT conversation_id FROM message WHERE account_id = ?1 AND id = ?2",
                params![account_id.as_str(), message.id.as_str()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(sql_to_store_error)?
            .flatten()
            .map(ConversationId);
        let existed = tx
            .query_row(
                "SELECT 1 FROM message WHERE account_id = ?1 AND id = ?2",
                params![account_id.as_str(), message.id.as_str()],
                |_row| Ok(()),
            )
            .optional()
            .map_err(sql_to_store_error)?
            .is_some();
        let conversation_id = assign_conversation_id_tx(tx, account_id, message)?;

        tx.execute(
            "INSERT INTO message (
                account_id, id, thread_id, conversation_id, remote_blob_id, subject,
                normalized_subject, from_name, from_email, preview, received_at,
                has_attachment, size, is_read, is_flagged, rfc_message_id, in_reply_to,
                references_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
             ON CONFLICT(account_id, id) DO UPDATE SET
                thread_id = excluded.thread_id,
                conversation_id = excluded.conversation_id,
                remote_blob_id = excluded.remote_blob_id,
                subject = excluded.subject,
                normalized_subject = excluded.normalized_subject,
                from_name = excluded.from_name,
                from_email = excluded.from_email,
                preview = excluded.preview,
                received_at = excluded.received_at,
                has_attachment = excluded.has_attachment,
                size = excluded.size,
                is_read = excluded.is_read,
                is_flagged = excluded.is_flagged,
                rfc_message_id = excluded.rfc_message_id,
                in_reply_to = excluded.in_reply_to,
                references_json = excluded.references_json",
            params![
                account_id.as_str(),
                message.id.as_str(),
                message.source_thread_id.as_str(),
                conversation_id.as_str(),
                message.remote_blob_id.as_ref().map(|blob_id| blob_id.as_str()),
                message.subject,
                normalized_subject(message.subject.as_deref()),
                message.from_name,
                message.from_email,
                message.preview,
                message.received_at,
                bool_to_i64(message.has_attachment),
                message.size,
                bool_to_i64(message.keywords.iter().any(|keyword| keyword == "$seen")),
                bool_to_i64(message.keywords.iter().any(|keyword| keyword == "$flagged")),
                message.rfc_message_id,
                message.in_reply_to,
                serde_json::to_string(&message.references).map_err(json_to_store_error)?
            ],
        )
        .map_err(sql_to_store_error)?;

        tx.execute(
            "DELETE FROM conversation_message WHERE account_id = ?1 AND message_id = ?2",
            params![account_id.as_str(), message.id.as_str()],
        )
        .map_err(sql_to_store_error)?;
        tx.execute(
            "INSERT INTO conversation_message (conversation_id, account_id, message_id)
             VALUES (?1, ?2, ?3)",
            params![
                conversation_id.as_str(),
                account_id.as_str(),
                message.id.as_str()
            ],
        )
        .map_err(sql_to_store_error)?;

        tx.execute(
            "DELETE FROM message_mailbox WHERE account_id = ?1 AND message_id = ?2",
            params![account_id.as_str(), message.id.as_str()],
        )
        .map_err(sql_to_store_error)?;
        for mailbox_id in &message.mailbox_ids {
            tx.execute(
                "INSERT INTO message_mailbox (account_id, message_id, mailbox_id)
                 VALUES (?1, ?2, ?3)",
                params![
                    account_id.as_str(),
                    message.id.as_str(),
                    mailbox_id.as_str()
                ],
            )
            .map_err(sql_to_store_error)?;
        }

        tx.execute(
            "DELETE FROM message_keyword WHERE account_id = ?1 AND message_id = ?2",
            params![account_id.as_str(), message.id.as_str()],
        )
        .map_err(sql_to_store_error)?;
        for keyword in &message.keywords {
            tx.execute(
                "INSERT INTO message_keyword (account_id, message_id, keyword)
                 VALUES (?1, ?2, ?3)",
                params![account_id.as_str(), message.id.as_str(), keyword],
            )
            .map_err(sql_to_store_error)?;
        }

        if message.body_html.is_some() || message.body_text.is_some() || raw_ref.is_some() {
            upsert_body_tx(
                tx,
                account_id,
                &message.id,
                message.body_html.as_deref(),
                message.body_text.as_deref(),
                raw_ref.as_ref(),
            )?;
        }

        affected_threads.insert(message.source_thread_id.clone());
        affected_conversations.insert(conversation_id.clone());
        if let Some(previous_conversation_id) = previous_conversation_id {
            affected_conversations.insert(previous_conversation_id);
        }
        for mailbox_id in previous_mailboxes.iter().chain(message.mailbox_ids.iter()) {
            affected_mailboxes.insert(mailbox_id.clone());
        }

        events.push(insert_event_tx(
            tx,
            account_id,
            EVENT_TOPIC_MESSAGE_UPDATED,
            message.mailbox_ids.first(),
            Some(&message.id),
            json!({
                "messageId": message.id.as_str(),
                "sourceThreadId": message.source_thread_id.as_str(),
                "conversationId": conversation_id.as_str(),
                "created": !existed
            }),
        )?);

        if !existed || previous_keywords != message.keywords {
            events.push(insert_event_tx(
                tx,
                account_id,
                "message.keywords_changed",
                message.mailbox_ids.first(),
                Some(&message.id),
                json!({
                    "messageId": message.id.as_str(),
                    "keywords": message.keywords,
                }),
            )?);
        }

        let current_mailboxes: BTreeSet<_> = message.mailbox_ids.iter().cloned().collect();
        let previous_mailboxes_set: BTreeSet<_> = previous_mailboxes.iter().cloned().collect();
        if !existed || current_mailboxes != previous_mailboxes_set {
            events.push(insert_event_tx(
                tx,
                account_id,
                "message.mailboxes_changed",
                message.mailbox_ids.first(),
                Some(&message.id),
                json!({
                    "messageId": message.id.as_str(),
                    "mailboxIds": message.mailbox_ids.iter().map(MailboxId::as_str).collect::<Vec<_>>(),
                }),
            )?);
        }

        for mailbox_id in current_mailboxes.difference(&previous_mailboxes_set) {
            events.push(insert_event_tx(
                tx,
                account_id,
                EVENT_TOPIC_MESSAGE_ARRIVED,
                Some(mailbox_id),
                Some(&message.id),
                json!({
                    "messageId": message.id.as_str(),
                    "mailboxId": mailbox_id.as_str(),
                }),
            )?);
        }
    }

    for location in &batch.imap_message_locations {
        tx.execute(
            "INSERT INTO imap_message_location (
                account_id, message_id, mailbox_id, uid_validity, uid, modseq, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(account_id, message_id, mailbox_id) DO UPDATE SET
                uid_validity = excluded.uid_validity,
                uid = excluded.uid,
                modseq = excluded.modseq,
                updated_at = excluded.updated_at",
            params![
                account_id.as_str(),
                location.message_id.as_str(),
                location.mailbox_id.as_str(),
                location.uid_validity.0,
                location.uid.0,
                location.modseq.map(|modseq| modseq.0.to_string()),
                location.updated_at,
            ],
        )
        .map_err(sql_to_store_error)?;
    }

    for state in &batch.imap_mailbox_states {
        tx.execute(
            "INSERT INTO imap_mailbox_sync_state (
                account_id, mailbox_id, mailbox_name, uid_validity,
                highest_uid, highest_modseq, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(account_id, mailbox_id) DO UPDATE SET
                mailbox_name = excluded.mailbox_name,
                uid_validity = excluded.uid_validity,
                highest_uid = excluded.highest_uid,
                highest_modseq = excluded.highest_modseq,
                updated_at = excluded.updated_at",
            params![
                account_id.as_str(),
                state.mailbox_id.as_str(),
                state.mailbox_name,
                state.uid_validity.0,
                state.highest_uid.map(|uid| uid.0),
                state.highest_modseq.map(|modseq| modseq.0.to_string()),
                state.updated_at,
            ],
        )
        .map_err(sql_to_store_error)?;
    }

    for thread_id in affected_threads {
        refresh_thread_projection_tx(tx, account_id, &thread_id)?;
    }
    for conversation_id in affected_conversations {
        refresh_conversation_projection_tx(tx, &conversation_id)?;
    }
    for mailbox_id in affected_mailboxes {
        refresh_mailbox_counters_tx(tx, account_id, &mailbox_id)?;
    }
    for cursor in &batch.cursors {
        DatabaseStore::upsert_sync_cursor_tx(tx, account_id, cursor)?;
    }

    Ok(events)
}

fn delete_message_and_track_projection_inputs(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    affected_mailboxes: &mut BTreeSet<MailboxId>,
    affected_threads: &mut BTreeSet<ThreadId>,
    affected_conversations: &mut BTreeSet<ConversationId>,
) -> Result<(), StoreError> {
    let prior_mailboxes = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
    let thread_id = tx
        .query_row(
            "SELECT thread_id FROM message WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), message_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_to_store_error)?
        .map(ThreadId);
    let conversation_id = tx
        .query_row(
            "SELECT conversation_id FROM message WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), message_id.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(sql_to_store_error)?
        .flatten()
        .map(ConversationId);
    delete_message_tx(tx, account_id, message_id)?;
    for mailbox_id in prior_mailboxes {
        affected_mailboxes.insert(mailbox_id);
    }
    if let Some(thread_id) = thread_id {
        affected_threads.insert(thread_id);
    }
    if let Some(conversation_id) = conversation_id {
        affected_conversations.insert(conversation_id);
    }
    Ok(())
}

/// Stores a lazily fetched body (HTML, text, raw ref) and emits a
/// `message.body_cached` event. Returns the updated message detail.
///
/// @spec docs/L1-sync#invariants
pub(crate) fn apply_message_body_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    body: &FetchedBody,
    raw_ref: Option<&RawMessageRef>,
) -> Result<CommandResult, StoreError> {
    upsert_body_tx(
        tx,
        account_id,
        message_id,
        body.body_html.as_deref(),
        body.body_text.as_deref(),
        raw_ref,
    )?;
    replace_attachments_tx(tx, account_id, message_id, &body.attachments)?;
    let event = insert_event_tx(
        tx,
        account_id,
        "message.body_cached",
        None,
        Some(message_id),
        json!({ "messageId": message_id.as_str() }),
    )?;
    let detail = query_message_detail_tx(tx, account_id, message_id)?
        .ok_or_else(|| StoreError::NotFound(format!("message:{}", message_id.as_str())))?;
    Ok(CommandResult {
        detail: Some(detail),
        events: vec![event],
    })
}

/// Adds and removes keywords on a message, updates the `is_read`/`is_flagged`
/// denormalized columns, refreshes mailbox counters, and emits a
/// `message.keywords_changed` event.
pub(crate) fn set_keywords_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    cursor: Option<&SyncCursor>,
    command: &SetKeywordsCommand,
) -> Result<CommandResult, StoreError> {
    let existing_keywords = fetch_keywords_tx(tx, account_id, message_id)?;
    let mut keywords: BTreeSet<_> = existing_keywords.into_iter().collect();
    for keyword in &command.add {
        keywords.insert(keyword.clone());
    }
    for keyword in &command.remove {
        keywords.remove(keyword);
    }
    tx.execute(
        "DELETE FROM message_keyword WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    for keyword in &keywords {
        tx.execute(
            "INSERT INTO message_keyword (account_id, message_id, keyword) VALUES (?1, ?2, ?3)",
            params![account_id.as_str(), message_id.as_str(), keyword],
        )
        .map_err(sql_to_store_error)?;
    }
    tx.execute(
        "UPDATE message
         SET is_read = ?3, is_flagged = ?4
         WHERE account_id = ?1 AND id = ?2",
        params![
            account_id.as_str(),
            message_id.as_str(),
            bool_to_i64(keywords.contains("$seen")),
            bool_to_i64(keywords.contains("$flagged"))
        ],
    )
    .map_err(sql_to_store_error)?;

    let mailboxes = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
    for mailbox_id in &mailboxes {
        refresh_mailbox_counters_tx(tx, account_id, mailbox_id)?;
    }
    let event = insert_event_tx(
        tx,
        account_id,
        "message.keywords_changed",
        mailboxes.first(),
        Some(message_id),
        json!({ "messageId": message_id.as_str(), "keywords": keywords.iter().cloned().collect::<Vec<_>>() }),
    )?;
    if let Some(cursor) = cursor {
        DatabaseStore::upsert_sync_cursor_tx(tx, account_id, cursor)?;
    }
    let detail = query_message_detail_tx(tx, account_id, message_id)?
        .ok_or_else(|| StoreError::NotFound(format!("message:{}", message_id.as_str())))?;
    Ok(CommandResult {
        detail: Some(detail),
        events: vec![event],
    })
}

/// Replaces a message's mailbox memberships. Refreshes counters for both old
/// and new mailboxes, emits `message.mailboxes_changed` and
/// `message.arrived` events for newly added mailboxes.
pub(crate) fn replace_mailboxes_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    cursor: Option<&SyncCursor>,
    command: &ReplaceMailboxesCommand,
) -> Result<CommandResult, StoreError> {
    let previous_mailboxes = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
    tx.execute(
        "DELETE FROM message_mailbox WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    for mailbox_id in &command.mailbox_ids {
        tx.execute(
            "INSERT INTO message_mailbox (account_id, message_id, mailbox_id) VALUES (?1, ?2, ?3)",
            params![
                account_id.as_str(),
                message_id.as_str(),
                mailbox_id.as_str()
            ],
        )
        .map_err(sql_to_store_error)?;
    }

    let previous_set: BTreeSet<_> = previous_mailboxes.iter().cloned().collect();
    let current_set: BTreeSet<_> = command.mailbox_ids.iter().cloned().collect();

    for mailbox_id in previous_set.union(&current_set) {
        refresh_mailbox_counters_tx(tx, account_id, mailbox_id)?;
    }

    let mut events = Vec::new();
    events.push(insert_event_tx(
        tx,
        account_id,
        "message.mailboxes_changed",
        command.mailbox_ids.first(),
        Some(message_id),
        json!({
            "messageId": message_id.as_str(),
            "mailboxIds": command.mailbox_ids.iter().map(MailboxId::as_str).collect::<Vec<_>>(),
        }),
    )?);
    for mailbox_id in current_set.difference(&previous_set) {
        events.push(insert_event_tx(
            tx,
            account_id,
            EVENT_TOPIC_MESSAGE_ARRIVED,
            Some(mailbox_id),
            Some(message_id),
            json!({ "messageId": message_id.as_str(), "mailboxId": mailbox_id.as_str() }),
        )?);
    }

    if let Some(cursor) = cursor {
        DatabaseStore::upsert_sync_cursor_tx(tx, account_id, cursor)?;
    }
    let detail = query_message_detail_tx(tx, account_id, message_id)?
        .ok_or_else(|| StoreError::NotFound(format!("message:{}", message_id.as_str())))?;
    Ok(CommandResult {
        detail: Some(detail),
        events,
    })
}

/// Deletes a message and all junction rows, refreshes thread/mailbox
/// projections, and emits a deletion event.
pub(crate) fn destroy_message_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    cursor: Option<&SyncCursor>,
) -> Result<CommandResult, StoreError> {
    let previous_mailboxes = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
    let thread_id = tx
        .query_row(
            "SELECT thread_id FROM message WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), message_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_to_store_error)?
        .map(ThreadId)
        .ok_or_else(|| StoreError::NotFound(format!("message:{}", message_id.as_str())))?;
    delete_message_tx(tx, account_id, message_id)?;
    refresh_thread_projection_tx(tx, account_id, &thread_id)?;
    for mailbox_id in &previous_mailboxes {
        refresh_mailbox_counters_tx(tx, account_id, mailbox_id)?;
    }
    let event = insert_event_tx(
        tx,
        account_id,
        EVENT_TOPIC_MESSAGE_UPDATED,
        previous_mailboxes.first(),
        Some(message_id),
        json!({ "messageId": message_id.as_str(), "deleted": true }),
    )?;
    if let Some(cursor) = cursor {
        DatabaseStore::upsert_sync_cursor_tx(tx, account_id, cursor)?;
    }
    Ok(CommandResult {
        detail: None,
        events: vec![event],
    })
}

/// Queries the `event_log` table with optional filters (account, seq cursor,
/// topic, mailbox). Returns events ordered by `seq ASC`.
///
/// @spec docs/L1-sync#event-propagation
pub(crate) fn list_events(
    connection: &Connection,
    filter: &EventFilter,
) -> Result<Vec<DomainEvent>, StoreError> {
    let mut sql = "SELECT seq, account_id, topic, occurred_at, mailbox_id, message_id, payload
         FROM event_log
         WHERE 1 = 1"
        .to_string();
    let mut bindings: Vec<SqlValue> = Vec::new();

    if let Some(account_id) = &filter.account_id {
        sql.push_str(" AND account_id = ?");
        sql.push_str(&(bindings.len() + 1).to_string());
        bindings.push(SqlValue::Text(account_id.to_string()));
    }

    if let Some(after_seq) = filter.after_seq {
        sql.push_str(" AND seq > ?");
        sql.push_str(&(bindings.len() + 1).to_string());
        bindings.push(SqlValue::Integer(after_seq));
    }
    if let Some(topic) = &filter.topic {
        sql.push_str(" AND topic = ?");
        sql.push_str(&(bindings.len() + 1).to_string());
        bindings.push(SqlValue::Text(topic.clone()));
    }
    if let Some(mailbox_id) = &filter.mailbox_id {
        sql.push_str(" AND mailbox_id = ?");
        sql.push_str(&(bindings.len() + 1).to_string());
        bindings.push(SqlValue::Text(mailbox_id.to_string()));
    }
    sql.push_str(" ORDER BY seq ASC");

    let mut statement = connection.prepare(&sql).map_err(sql_to_store_error)?;
    let params_ref = rusqlite::params_from_iter(bindings);
    let rows = statement
        .query_map(params_ref, row_to_event)
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}
