use super::*;

/// Stores a lazily fetched body (HTML, text, raw ref), emits
/// `EVENT_TOPIC_MESSAGE_BODY_CACHED`, and returns the updated message detail.
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
    ensure_body_cache_object_tx(
        tx,
        account_id,
        message_id,
        true,
        "body-cached",
        BACKGROUND_RESCORE_PRIORITY,
    )?;
    let event = insert_event_tx(
        tx,
        account_id,
        EVENT_TOPIC_MESSAGE_BODY_CACHED,
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
/// `EVENT_TOPIC_MESSAGE_KEYWORDS_CHANGED` event.
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
    let detail = query_message_detail_tx(tx, account_id, message_id)?
        .ok_or_else(|| StoreError::NotFound(format!("message:{}", message_id.as_str())))?;
    let assertion = posthaste_domain::MessageChangeAssertion::after(detail.summary.clone());
    let event = insert_event_tx(
        tx,
        account_id,
        EVENT_TOPIC_MESSAGE_KEYWORDS_CHANGED,
        mailboxes.first(),
        Some(message_id),
        json!({
            "messageId": message_id.as_str(),
            "keywords": keywords.iter().cloned().collect::<Vec<_>>(),
            "assertion": assertion,
        }),
    )?;
    if let Some(cursor) = cursor {
        DatabaseStore::upsert_sync_cursor_tx(tx, account_id, cursor)?;
    }
    Ok(CommandResult {
        detail: Some(detail),
        events: vec![event],
    })
}

/// Replaces a message's mailbox memberships. Refreshes counters for both old
/// and new mailboxes, emits `EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED` and
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

    let mut events = Vec::new();
    events.push(insert_event_tx(
        tx,
        account_id,
        EVENT_TOPIC_MESSAGE_MAILBOXES_CHANGED,
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
