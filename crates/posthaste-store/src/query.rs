use super::*;
use posthaste_domain::{BlobId, MessageAttachment};

fn row_to_message_attachment(
    row: &rusqlite::Row<'_>,
) -> Result<MessageAttachment, rusqlite::Error> {
    Ok(MessageAttachment {
        id: row.get(0)?,
        blob_id: BlobId(row.get(1)?),
        part_id: row.get(2)?,
        filename: row.get(3)?,
        mime_type: row.get(4)?,
        size: row.get(5)?,
        disposition: row.get(6)?,
        cid: row.get(7)?,
        is_inline: row.get::<_, i64>(8)? != 0,
    })
}

pub(crate) fn fetch_message_attachments(
    connection: &Connection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<MessageAttachment>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT id, blob_id, part_id, filename, mime_type, size, disposition, cid, is_inline
             FROM message_attachment
             WHERE account_id = ?1 AND message_id = ?2
             ORDER BY id ASC",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(
            params![account_id.as_str(), message_id.as_str()],
            row_to_message_attachment,
        )
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}

pub(crate) fn fetch_message_attachments_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<MessageAttachment>, StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT id, blob_id, part_id, filename, mime_type, size, disposition, cid, is_inline
             FROM message_attachment
             WHERE account_id = ?1 AND message_id = ?2
             ORDER BY id ASC",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(
            params![account_id.as_str(), message_id.as_str()],
            row_to_message_attachment,
        )
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}

/// Intermediate row from a message summary query, before hydration with
/// mailbox IDs and keywords.
#[derive(Debug)]
pub(crate) struct MessageSummaryRow {
    pub(crate) id: MessageId,
    pub(crate) source_id: AccountId,
    pub(crate) source_name: String,
    pub(crate) source_thread_id: ThreadId,
    pub(crate) conversation_id: ConversationId,
    pub(crate) subject: Option<String>,
    pub(crate) from_name: Option<String>,
    pub(crate) from_email: Option<String>,
    pub(crate) preview: Option<String>,
    pub(crate) received_at: String,
    pub(crate) has_attachment: bool,
    pub(crate) is_read: bool,
    pub(crate) is_flagged: bool,
}

/// Executes a prepared message summary statement and collects the rows.
pub(crate) fn load_message_summary_rows<P: rusqlite::Params>(
    statement: &mut rusqlite::Statement<'_>,
    params: P,
) -> Result<Vec<MessageSummaryRow>, StoreError> {
    let rows = statement
        .query_map(params, row_to_message_summary_row)
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}

/// Hydrates intermediate message rows with mailbox IDs and keywords via bulk
/// lookups, preserving the original row order.
pub(crate) fn hydrate_message_summaries(
    connection: &Connection,
    rows: Vec<MessageSummaryRow>,
) -> Result<Vec<MessageSummary>, StoreError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let mailbox_ids = fetch_mailbox_ids_bulk(connection, &rows)?;
    let keywords = fetch_keywords_bulk(connection, &rows)?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let key = (
                row.source_id.as_str().to_string(),
                row.id.as_str().to_string(),
            );
            MessageSummary {
                id: row.id,
                source_id: row.source_id,
                source_name: row.source_name,
                source_thread_id: row.source_thread_id,
                conversation_id: row.conversation_id,
                subject: row.subject,
                from_name: row.from_name,
                from_email: row.from_email,
                preview: row.preview,
                received_at: row.received_at,
                has_attachment: row.has_attachment,
                is_read: row.is_read,
                is_flagged: row.is_flagged,
                mailbox_ids: mailbox_ids.get(&key).cloned().unwrap_or_default(),
                keywords: keywords.get(&key).cloned().unwrap_or_default(),
            }
        })
        .collect())
}

/// Maps an `event_log` row to a `DomainEvent`.
pub(crate) fn row_to_event(row: &rusqlite::Row<'_>) -> Result<DomainEvent, rusqlite::Error> {
    let payload: String = row.get(6)?;
    Ok(DomainEvent {
        seq: row.get(0)?,
        account_id: AccountId(row.get(1)?),
        topic: row.get(2)?,
        occurred_at: row.get(3)?,
        mailbox_id: row.get::<_, Option<String>>(4)?.map(MailboxId),
        message_id: row.get::<_, Option<String>>(5)?.map(MessageId),
        payload: serde_json::from_str(&payload).unwrap_or_else(|_| json!({})),
    })
}

/// Fetches mailbox IDs for a single message (read connection).
pub(crate) fn fetch_mailbox_ids(
    connection: &Connection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<MailboxId>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT mailbox_id
             FROM message_mailbox
             WHERE account_id = ?1 AND message_id = ?2
             ORDER BY mailbox_id",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            Ok(MailboxId(row.get(0)?))
        })
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}

/// Fetches mailbox IDs for a single message (within a transaction).
pub(crate) fn fetch_mailbox_ids_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<MailboxId>, StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT mailbox_id
             FROM message_mailbox
             WHERE account_id = ?1 AND message_id = ?2
             ORDER BY mailbox_id",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            Ok(MailboxId(row.get(0)?))
        })
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}

/// Fetches keywords for a single message (within a transaction).
pub(crate) fn fetch_keywords_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<String>, StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT keyword
             FROM message_keyword
             WHERE account_id = ?1 AND message_id = ?2
             ORDER BY keyword",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            row.get(0)
        })
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}

/// Fetches a single message's full detail (summary + body + raw ref) within
/// a transaction.
pub(crate) fn query_message_detail_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<MessageDetail>, StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT m.id, m.account_id, a.name, m.thread_id, m.conversation_id, m.subject,
                    m.from_name, m.from_email, m.preview, m.received_at, m.has_attachment,
                    m.is_read, m.is_flagged
             FROM message m
             JOIN source_projection a
               ON a.source_id = m.account_id
             WHERE m.account_id = ?1 AND m.id = ?2",
        )
        .map_err(sql_to_store_error)?;

    let detail = statement
        .query_row(params![account_id.as_str(), message_id.as_str()], |row| {
            Ok(MessageSummary {
                id: MessageId(row.get(0)?),
                source_id: AccountId(row.get(1)?),
                source_name: row.get(2)?,
                source_thread_id: ThreadId(row.get(3)?),
                conversation_id: ConversationId(row.get(4)?),
                subject: row.get(5)?,
                from_name: row.get(6)?,
                from_email: row.get(7)?,
                preview: row.get(8)?,
                received_at: row.get(9)?,
                has_attachment: row.get::<_, i64>(10)? != 0,
                is_read: row.get::<_, i64>(11)? != 0,
                is_flagged: row.get::<_, i64>(12)? != 0,
                mailbox_ids: Vec::new(),
                keywords: Vec::new(),
            })
        })
        .optional()
        .map_err(sql_to_store_error)?;

    let Some(mut summary) = detail else {
        return Ok(None);
    };

    summary.mailbox_ids = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
    summary.keywords = fetch_keywords_tx(tx, account_id, message_id)?;

    let body = tx
        .query_row(
            "SELECT body_html, body_text, raw_path, raw_sha256, raw_size, raw_mime_type, fetched_at
             FROM message_body
             WHERE account_id = ?1 AND message_id = ?2",
            params![account_id.as_str(), message_id.as_str()],
            |row| {
                let raw_path: Option<String> = row.get(2)?;
                let raw_sha256: Option<String> = row.get(3)?;
                let raw_size: Option<i64> = row.get(4)?;
                let raw_mime_type: Option<String> = row.get(5)?;
                let fetched_at: Option<String> = row.get(6)?;
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    raw_path.and_then(|path| {
                        Some(RawMessageRef {
                            path,
                            sha256: raw_sha256?,
                            size: raw_size?,
                            mime_type: raw_mime_type?,
                            fetched_at: fetched_at?,
                        })
                    }),
                ))
            },
        )
        .optional()
        .map_err(sql_to_store_error)?;
    let attachments = fetch_message_attachments_tx(tx, account_id, message_id)?;

    Ok(Some(MessageDetail {
        summary,
        body_html: body.as_ref().and_then(|tuple| tuple.0.clone()),
        body_text: body.as_ref().and_then(|tuple| tuple.1.clone()),
        raw_message: body.and_then(|tuple| tuple.2),
        attachments,
    }))
}

/// Maps a database row to a `MessageSummaryRow`.
pub(crate) fn row_to_message_summary_row(
    row: &rusqlite::Row<'_>,
) -> Result<MessageSummaryRow, rusqlite::Error> {
    Ok(MessageSummaryRow {
        id: MessageId(row.get(0)?),
        source_id: AccountId(row.get(1)?),
        source_name: row.get(2)?,
        source_thread_id: ThreadId(row.get(3)?),
        conversation_id: ConversationId(row.get(4)?),
        subject: row.get(5)?,
        from_name: row.get(6)?,
        from_email: row.get(7)?,
        preview: row.get(8)?,
        received_at: row.get(9)?,
        has_attachment: row.get::<_, i64>(10)? != 0,
        is_read: row.get::<_, i64>(11)? != 0,
        is_flagged: row.get::<_, i64>(12)? != 0,
    })
}

/// Bulk-fetches mailbox IDs for a set of messages in chunks.
fn fetch_mailbox_ids_bulk(
    connection: &Connection,
    rows: &[MessageSummaryRow],
) -> Result<HashMap<(String, String), Vec<MailboxId>>, StoreError> {
    fetch_message_values_bulk(connection, rows, "message_mailbox", "mailbox_id", |row| {
        Ok(MailboxId(row.get(2)?))
    })
}

/// Bulk-fetches keywords for a set of messages in chunks.
fn fetch_keywords_bulk(
    connection: &Connection,
    rows: &[MessageSummaryRow],
) -> Result<HashMap<(String, String), Vec<String>>, StoreError> {
    fetch_message_values_bulk(connection, rows, "message_keyword", "keyword", |row| {
        row.get(2)
    })
}

/// Generic bulk-fetch for message-associated values (mailbox IDs or keywords).
/// Queries in chunks of 400 to avoid SQLite parameter limits.
fn fetch_message_values_bulk<T>(
    connection: &Connection,
    rows: &[MessageSummaryRow],
    table: &str,
    value_column: &str,
    mut map_value: impl FnMut(&rusqlite::Row<'_>) -> Result<T, rusqlite::Error>,
) -> Result<HashMap<(String, String), Vec<T>>, StoreError> {
    const CHUNK_SIZE: usize = 400;

    let mut seen = HashSet::new();
    let mut keys = Vec::new();
    for row in rows {
        let key = (
            row.source_id.as_str().to_string(),
            row.id.as_str().to_string(),
        );
        if seen.insert(key.clone()) {
            keys.push(key);
        }
    }

    let mut values_by_key = HashMap::new();
    for chunk in keys.chunks(CHUNK_SIZE) {
        let mut params = Vec::with_capacity(chunk.len() * 2);
        let mut predicates = Vec::with_capacity(chunk.len());
        for (account_id, message_id) in chunk {
            predicates.push("(account_id = ? AND message_id = ?)".to_string());
            params.push(SqlValue::Text(account_id.clone()));
            params.push(SqlValue::Text(message_id.clone()));
        }
        let sql = format!(
            "SELECT account_id, message_id, {value_column}
             FROM {table}
             WHERE {}
             ORDER BY account_id, message_id, {value_column}",
            predicates.join(" OR ")
        );
        let mut statement = connection.prepare(&sql).map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params_from_iter(params), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    map_value(row)?,
                ))
            })
            .map_err(sql_to_store_error)?;
        for row in rows {
            let (account_id, message_id, value) = row.map_err(sql_to_store_error)?;
            values_by_key
                .entry((account_id, message_id))
                .or_insert_with(Vec::new)
                .push(value);
        }
    }

    Ok(values_by_key)
}
