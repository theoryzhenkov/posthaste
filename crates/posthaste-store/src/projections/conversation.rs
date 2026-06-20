use super::*;
use crate::sql_cache::CachedSql;

/// Determines the conversation ID for a message from JMAP `threadId`.
///
/// JMAP owns thread membership. RFC headers and subject normalization are
/// retained as display/search metadata, but they do not define conversation
/// membership.
///
/// @spec docs/L1-sync#sqlite-schema
pub(crate) fn assign_conversation_id_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message: &posthaste_domain::MessageRecord,
) -> Result<ConversationId, StoreError> {
    if let Some(conversation_id) = tx
        .query_row_cached(
            "SELECT conversation_id
             FROM message
             WHERE account_id = ?1 AND thread_id = ?2 AND conversation_id IS NOT NULL
             ORDER BY received_at DESC
             LIMIT 1",
            params![account_id.as_str(), message.source_thread_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_to_store_error)?
    {
        return Ok(ConversationId::from(conversation_id.as_str()));
    }

    Ok(generate_conversation_id(account_id, message))
}

/// Recomputes the `conversation` projection row (subject, latest message,
/// counts) from the linked messages. Deletes the row if no messages remain.
///
/// @spec docs/L1-sync#sqlite-schema
pub(crate) fn refresh_conversation_projection_tx(
    tx: &Transaction<'_>,
    conversation_id: &ConversationId,
) -> Result<(), StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT m.account_id, m.id, m.subject, m.normalized_subject, m.received_at, m.is_read
             FROM conversation_message cm
             JOIN message m
               ON m.account_id = cm.account_id
              AND m.id = cm.message_id
             WHERE cm.conversation_id = ?1
             ORDER BY m.received_at DESC, m.id DESC",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![conversation_id.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;

    if rows.is_empty() {
        tx.execute_cached(
            "DELETE FROM conversation WHERE id = ?1",
            params![conversation_id.as_str()],
        )
        .map_err(sql_to_store_error)?;
        return Ok(());
    }

    let latest = &rows[0];
    let subject = latest
        .2
        .clone()
        .or_else(|| rows.iter().find_map(|row| row.2.clone()));
    let normalized_subject_value = latest
        .3
        .clone()
        .or_else(|| rows.iter().find_map(|row| row.3.clone()));
    let unread_count = rows.iter().filter(|row| row.5 == 0).count() as i64;
    tx.execute_cached(
        "INSERT INTO conversation (
            id, subject, normalized_subject, latest_received_at, latest_source_id,
            latest_message_id, message_count, unread_count
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
            subject = excluded.subject,
            normalized_subject = excluded.normalized_subject,
            latest_received_at = excluded.latest_received_at,
            latest_source_id = excluded.latest_source_id,
            latest_message_id = excluded.latest_message_id,
            message_count = excluded.message_count,
            unread_count = excluded.unread_count",
        params![
            conversation_id.as_str(),
            subject,
            normalized_subject_value,
            &latest.4,
            &latest.0,
            &latest.1,
            rows.len() as i64,
            unread_count,
        ],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

/// Removes conversation rows that have no linked messages.
pub(crate) fn cleanup_orphan_conversations_tx(tx: &Transaction<'_>) -> Result<(), StoreError> {
    tx.execute_cached(
        "DELETE FROM conversation
         WHERE id NOT IN (SELECT DISTINCT conversation_id FROM conversation_message)",
        [],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

pub(crate) fn normalized_subject(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let mut normalized = value.trim();
        loop {
            let lower = normalized.to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("re:") {
                normalized = normalized[normalized.len() - rest.len()..].trim();
                continue;
            }
            if let Some(rest) = lower.strip_prefix("fwd:") {
                normalized = normalized[normalized.len() - rest.len()..].trim();
                continue;
            }
            break;
        }
        if normalized.is_empty() {
            None
        } else {
            Some(normalized.to_ascii_lowercase())
        }
    })
}

/// Generates a deterministic conversation ID from account ID and JMAP
/// `threadId` via SHA-256.
fn generate_conversation_id(
    account_id: &AccountId,
    message: &posthaste_domain::MessageRecord,
) -> ConversationId {
    let mut hasher = Sha256::new();
    hasher.update(account_id.as_str().as_bytes());
    hasher.update(message.source_thread_id.as_str().as_bytes());
    ConversationId(format!("conv-{}", hex_encode(hasher.finalize())))
}
