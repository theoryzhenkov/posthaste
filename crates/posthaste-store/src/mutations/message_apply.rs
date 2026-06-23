use super::*;
use crate::sql_cache::CachedSql;

pub(crate) fn effective_mailbox_role_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    mailbox_id: &MailboxId,
    discovered_role: Option<&str>,
) -> Result<Option<String>, StoreError> {
    let override_role = tx
        .query_row_cached(
            "SELECT role FROM mailbox_role_override
             WHERE account_id = ?1 AND mailbox_id = ?2",
            params![account_id.as_str(), mailbox_id.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(sql_to_store_error)?;

    Ok(override_role.unwrap_or_else(|| discovered_role.map(str::to_string)))
}

pub(crate) fn apply_message_record_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message: &posthaste_domain::MessageRecord,
    raw_ref: Option<&RawMessageRef>,
    affected: &mut ProjectionInputs,
    events: &mut EventRecorder<'_, '_, '_>,
) -> Result<(), StoreError> {
    let before = fetch_message_before_apply_tx(tx, account_id, &message.id)?;
    let conversation_id = assign_conversation_id_tx(tx, account_id, message)?;

    upsert_message_record_tx(tx, account_id, message, &conversation_id)?;
    replace_message_conversation_tx(tx, account_id, &message.id, &conversation_id)?;
    replace_message_mailboxes_tx(tx, account_id, &message.id, &message.mailbox_ids)?;
    replace_message_keywords_tx(tx, account_id, &message.id, &message.keywords)?;
    upsert_message_body_cache_tx(tx, account_id, message, raw_ref)?;

    track_applied_message_projection_inputs(affected, message, &conversation_id, &before);
    append_message_diff_events_tx(message, &conversation_id, &before, events)
}

pub(crate) fn fetch_message_before_apply_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<MessageBeforeApply, StoreError> {
    let mailboxes = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
    let keywords = fetch_keywords_tx(tx, account_id, message_id)?;
    let conversation_id = tx
        .query_row_cached(
            "SELECT conversation_id FROM message WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), message_id.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(sql_to_store_error)?
        .flatten()
        .map(ConversationId);
    let existed = tx
        .query_row_cached(
            "SELECT 1 FROM message WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), message_id.as_str()],
            |_row| Ok(()),
        )
        .optional()
        .map_err(sql_to_store_error)?
        .is_some();

    Ok(MessageBeforeApply {
        mailboxes,
        keywords,
        conversation_id,
        existed,
    })
}

pub(crate) fn upsert_message_record_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message: &posthaste_domain::MessageRecord,
    conversation_id: &ConversationId,
) -> Result<(), StoreError> {
    tx.execute_cached(
        "INSERT INTO message (
            account_id, id, thread_id, conversation_id, remote_blob_id, subject,
            normalized_subject, from_name, from_email, to_json, preview, received_at,
            has_attachment, size, is_read, is_flagged, rfc_message_id, in_reply_to,
            references_json, draft_id
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
         ON CONFLICT(account_id, id) DO UPDATE SET
            thread_id = excluded.thread_id,
            conversation_id = excluded.conversation_id,
            remote_blob_id = excluded.remote_blob_id,
            subject = excluded.subject,
            normalized_subject = excluded.normalized_subject,
            from_name = excluded.from_name,
            from_email = excluded.from_email,
            to_json = excluded.to_json,
            preview = excluded.preview,
            received_at = excluded.received_at,
            has_attachment = excluded.has_attachment,
            size = excluded.size,
            is_read = excluded.is_read,
            is_flagged = excluded.is_flagged,
            rfc_message_id = excluded.rfc_message_id,
            in_reply_to = excluded.in_reply_to,
            references_json = excluded.references_json,
            draft_id = excluded.draft_id",
        params![
            account_id.as_str(),
            message.id.as_str(),
            message.source_thread_id.as_str(),
            conversation_id.as_str(),
            message
                .remote_blob_id
                .as_ref()
                .map(|blob_id| blob_id.as_str()),
            message.subject,
            normalized_subject(message.subject.as_deref()),
            message.from_name,
            message.from_email,
            serde_json::to_string(&message.to).map_err(json_to_store_error)?,
            message.preview,
            message.received_at,
            bool_to_i64(message.has_attachment),
            message.size,
            bool_to_i64(message.keywords.iter().any(|keyword| keyword == "$seen")),
            bool_to_i64(message.keywords.iter().any(|keyword| keyword == "$flagged")),
            message.rfc_message_id,
            message.in_reply_to,
            serde_json::to_string(&message.references).map_err(json_to_store_error)?,
            message.draft_id
        ],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

pub(crate) fn replace_message_conversation_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    conversation_id: &ConversationId,
) -> Result<(), StoreError> {
    tx.execute_cached(
        "DELETE FROM conversation_message WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute_cached(
        "INSERT INTO conversation_message (conversation_id, account_id, message_id)
         VALUES (?1, ?2, ?3)",
        params![
            conversation_id.as_str(),
            account_id.as_str(),
            message_id.as_str()
        ],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

pub(crate) fn replace_message_mailboxes_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    mailbox_ids: &[MailboxId],
) -> Result<(), StoreError> {
    tx.execute_cached(
        "DELETE FROM message_mailbox WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    for mailbox_id in mailbox_ids {
        tx.execute_cached(
            "INSERT INTO message_mailbox (account_id, message_id, mailbox_id)
             VALUES (?1, ?2, ?3)",
            params![
                account_id.as_str(),
                message_id.as_str(),
                mailbox_id.as_str()
            ],
        )
        .map_err(sql_to_store_error)?;
    }
    Ok(())
}

pub(crate) fn replace_message_keywords_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    keywords: &[String],
) -> Result<(), StoreError> {
    tx.execute_cached(
        "DELETE FROM message_keyword WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    for keyword in keywords {
        tx.execute_cached(
            "INSERT INTO message_keyword (account_id, message_id, keyword)
             VALUES (?1, ?2, ?3)",
            params![account_id.as_str(), message_id.as_str(), keyword],
        )
        .map_err(sql_to_store_error)?;
    }
    Ok(())
}

pub(crate) fn upsert_message_body_cache_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message: &posthaste_domain::MessageRecord,
    raw_ref: Option<&RawMessageRef>,
) -> Result<(), StoreError> {
    let body_present =
        message.body_html.is_some() || message.body_text.is_some() || raw_ref.is_some();
    if body_present {
        upsert_body_tx(
            tx,
            account_id,
            &message.id,
            message.body_html.as_deref(),
            message.body_text.as_deref(),
            raw_ref,
        )?;
    }
    ensure_body_cache_object_tx(
        tx,
        account_id,
        &message.id,
        body_present,
        "metadata-sync",
        BACKGROUND_RESCORE_PRIORITY,
    )
}
