use super::*;
use crate::sql_cache::CachedSql;
#[cfg(test)]
use posthaste_domain_model::{ReplaceMailboxesCommand, SetKeywordsCommand};

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
    // Old-mail backfill: a body fetch re-serves the headers, so a message
    // ingested before the unsubscribe column existed gains its targets at
    // message-open. Non-clobbering — a value parsed at ingest wins.
    if let Some(list_unsubscribe) = &body.list_unsubscribe {
        tx.execute_cached(
            "UPDATE message SET list_unsubscribe = COALESCE(list_unsubscribe, ?3)
             WHERE account_id = ?1 AND id = ?2",
            params![
                account_id.as_str(),
                message_id.as_str(),
                serde_json::to_string(list_unsubscribe).map_err(json_to_store_error)?
            ],
        )
        .map_err(sql_to_store_error)?;
    }
    backfill_recipients_tx(tx, account_id, message_id, body)?;
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

/// Old-mail backfill for `cc`/`bcc`/`reply_to`, on the same footing as the
/// `list_unsubscribe` one above: a body fetch re-serves the full headers, so a
/// message ingested before these columns existed gains them at message-open
/// rather than needing a provider-wide resync (which delta sync would not do
/// anyway — it never re-fetches an unchanged message).
///
/// Only fills a column that is STILL EMPTY, and only from a non-empty fetched
/// value: an ingest-time parse always wins, and a fetch that carried nothing
/// can never blank a stored value.
fn backfill_recipients_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    body: &FetchedBody,
) -> Result<(), StoreError> {
    for (column, recipients) in [
        ("cc_json", &body.cc),
        ("bcc_json", &body.bcc),
        ("reply_to_json", &body.reply_to),
    ] {
        if recipients.is_empty() {
            continue;
        }
        tx.execute_cached(
            &format!(
                "UPDATE message SET {column} = ?3
                 WHERE account_id = ?1 AND id = ?2 AND {column} = '[]'"
            ),
            params![
                account_id.as_str(),
                message_id.as_str(),
                serde_json::to_string(recipients).map_err(json_to_store_error)?
            ],
        )
        .map_err(sql_to_store_error)?;
    }
    Ok(())
}

/// Adds and removes keywords on a message, updates the `is_read`/`is_flagged`
/// denormalized columns, and emits a coalesced `message.updated` metadata event.
/// TEST-ONLY since NS1 (the production write-through is deleted): store
/// tests use this to seed base keyword state directly. Follow-up: migrate
/// those tests to `apply_sync_batch` seeding and delete this.
#[cfg(test)]
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
    tx.execute_cached(
        "DELETE FROM message_keyword WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    for keyword in &keywords {
        tx.execute_cached(
            "INSERT INTO message_keyword (account_id, message_id, keyword) VALUES (?1, ?2, ?3)",
            params![account_id.as_str(), message_id.as_str(), keyword],
        )
        .map_err(sql_to_store_error)?;
    }
    tx.execute_cached(
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
    let assertion = posthaste_domain_model::MessageChangeAssertion::after(detail.summary.clone());
    // No counts on the event (RFC-L2-count-unification): clients react to the
    // event by INVALIDATING their mailbox-count query and re-reading the
    // trigger-maintained canonical counts, so the payload carries only the
    // row-liveness projection.
    let payload = json!({
        "messageId": message_id.as_str(),
        "changes": { "keywords": true },
        "keywords": keywords.iter().cloned().collect::<Vec<_>>(),
        "assertion": assertion,
        "projection": &detail.summary,
    });
    let event = insert_event_tx(
        tx,
        account_id,
        EVENT_TOPIC_MESSAGE_UPDATED,
        mailboxes.first(),
        Some(message_id),
        payload,
    )?;
    if let Some(cursor) = cursor {
        DatabaseStore::upsert_sync_cursor_tx(tx, account_id, cursor)?;
    }
    Ok(CommandResult {
        detail: Some(detail),
        events: vec![event],
    })
}

/// Replaces a message's mailbox memberships and emits one coalesced
/// `message.updated` metadata event.
/// TEST-ONLY since NS1 — see `set_keywords_tx`.
#[cfg(test)]
pub(crate) fn replace_mailboxes_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    cursor: Option<&SyncCursor>,
    command: &ReplaceMailboxesCommand,
) -> Result<CommandResult, StoreError> {
    let previous_mailboxes = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
    tx.execute_cached(
        "DELETE FROM message_mailbox WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    for mailbox_id in &command.mailbox_ids {
        tx.execute_cached(
            "INSERT INTO message_mailbox (account_id, message_id, mailbox_id) VALUES (?1, ?2, ?3)",
            params![
                account_id.as_str(),
                message_id.as_str(),
                mailbox_id.as_str()
            ],
        )
        .map_err(sql_to_store_error)?;
    }

    // Snooze invariant: any mailbox replace clears the snooze return-time row.
    // The `message.snooze` mutation re-inserts the row after its move, so this
    // only affects messages *leaving* the Snoozed mailbox (unsnooze, undo, a
    // manual move, the scheduler auto-return) — preventing orphaned rows. The
    // sync path does not route through here, so provider re-sync never clobbers
    // a snooze. @spec docs/eph/DESIGN-L2-snooze
    crate::snooze::clear_snooze_on_mailbox_replace_tx(tx, account_id, message_id)?;

    let previous_set: BTreeSet<_> = previous_mailboxes.iter().cloned().collect();
    let current_set: BTreeSet<_> = command.mailbox_ids.iter().cloned().collect();

    let arrived_mailbox_ids = current_set
        .difference(&previous_set)
        .map(MailboxId::as_str)
        .collect::<Vec<_>>();
    let detail = query_message_detail_tx(tx, account_id, message_id)?
        .ok_or_else(|| StoreError::NotFound(format!("message:{}", message_id.as_str())))?;
    let payload = json!({
        "messageId": message_id.as_str(),
        "changes": {
            "mailboxes": true,
            "arrived": !arrived_mailbox_ids.is_empty(),
        },
        "mailboxIds": command.mailbox_ids.iter().map(MailboxId::as_str).collect::<Vec<_>>(),
        "arrivedMailboxIds": arrived_mailbox_ids,
        "projection": &detail.summary,
    });
    let event = insert_event_tx(
        tx,
        account_id,
        EVENT_TOPIC_MESSAGE_UPDATED,
        command.mailbox_ids.first(),
        Some(message_id),
        payload,
    )?;

    if let Some(cursor) = cursor {
        DatabaseStore::upsert_sync_cursor_tx(tx, account_id, cursor)?;
    }
    Ok(CommandResult {
        detail: Some(detail),
        events: vec![event],
    })
}

/// Deletes a message and all junction rows, refreshes thread/mailbox
/// projections, and emits a deletion event.
#[cfg(test)]
pub(crate) fn destroy_message_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    cursor: Option<&SyncCursor>,
) -> Result<CommandResult, StoreError> {
    let previous_mailboxes = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
    let thread_id = tx
        .query_row_cached(
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
    let payload = json!({ "messageId": message_id.as_str(), "deleted": true });
    let event = insert_event_tx(
        tx,
        account_id,
        EVENT_TOPIC_MESSAGE_UPDATED,
        previous_mailboxes.first(),
        Some(message_id),
        payload,
    )?;
    if let Some(cursor) = cursor {
        DatabaseStore::upsert_sync_cursor_tx(tx, account_id, cursor)?;
    }
    Ok(CommandResult {
        detail: None,
        events: vec![event],
    })
}
