use super::*;

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
    pub(crate) to: Vec<Recipient>,
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

    let mut mailbox_ids = fetch_mailbox_ids_bulk(connection, &rows)?.into_iter();
    let mut keywords = fetch_keywords_bulk(connection, &rows)?.into_iter();
    let mut versions = fetch_versions_bulk(connection, &rows)?.into_iter();
    let mut threading = fetch_threading_bulk(connection, &rows)?.into_iter();

    Ok(rows
        .into_iter()
        .map(|row| {
            let (rfc_message_id, in_reply_to) = threading.next().unwrap_or((None, None));
            MessageSummary {
                id: row.id,
                source_id: row.source_id,
                source_name: row.source_name,
                source_thread_id: row.source_thread_id,
                conversation_id: row.conversation_id,
                subject: row.subject,
                from_name: row.from_name,
                from_email: row.from_email,
                to: row.to,
                preview: row.preview,
                received_at: row.received_at,
                has_attachment: row.has_attachment,
                is_read: row.is_read,
                is_flagged: row.is_flagged,
                mailbox_ids: mailbox_ids.next().unwrap_or_default(),
                keywords: keywords.next().unwrap_or_default(),
                version: versions.next().flatten(),
                rfc_message_id,
                in_reply_to,
            }
        })
        .collect())
}

/// Bulk-fetches the per-message authority version (max IMAP `modseq`, `CAST AS
/// INTEGER` since `modseq` is TEXT) for a set of messages, row-aligned. `None`
/// for a message with no IMAP location/modseq (JMAP / mock / local) — those
/// providers have no per-message version and the client leaves them unguarded.
/// @spec docs/eph/DESIGN-L2-message-authority-version
fn fetch_versions_bulk(
    connection: &Connection,
    rows: &[MessageSummaryRow],
) -> Result<Vec<Option<u64>>, StoreError> {
    const CHUNK_SIZE: usize = 300;

    let mut versions = vec![None; rows.len()];
    for (chunk_offset, chunk) in rows.chunks(CHUNK_SIZE).enumerate() {
        let start_index = chunk_offset * CHUNK_SIZE;
        let mut params = Vec::with_capacity(chunk.len() * 3);
        let mut values = Vec::with_capacity(chunk.len());
        for (offset, row) in chunk.iter().enumerate() {
            values.push("(?, ?, ?)".to_string());
            params.push(SqlValue::Integer((start_index + offset) as i64));
            params.push(SqlValue::Text(row.source_id.as_str().to_string()));
            params.push(SqlValue::Text(row.id.as_str().to_string()));
        }
        let sql = format!(
            "WITH requested(row_index, account_id, message_id) AS (VALUES {})
             SELECT requested.row_index, MAX(CAST(loc.modseq AS INTEGER))
               FROM requested
               LEFT JOIN imap_message_location loc
                 ON loc.account_id = requested.account_id
                AND loc.message_id = requested.message_id
                AND loc.modseq IS NOT NULL
              GROUP BY requested.row_index",
            values.join(", ")
        );
        let mut statement = connection
            .prepare_cached(&sql)
            .map_err(sql_to_store_error)?;
        let fetched = statement
            .query_map(params_from_iter(params), |row| {
                Ok((
                    row.get::<_, i64>(0)? as usize,
                    row.get::<_, Option<i64>>(1)?,
                ))
            })
            .map_err(sql_to_store_error)?;
        for entry in fetched {
            let (row_index, modseq) = entry.map_err(sql_to_store_error)?;
            versions[row_index] = modseq.map(|value| value as u64);
        }
    }

    Ok(versions)
}

/// A message's threading headers: `(rfc_message_id, in_reply_to)`, both
/// optional (`None` for a row with no stored header).
pub(crate) type ThreadingHeaders = (Option<String>, Option<String>);

/// Bulk-fetches the RFC `Message-ID` and `In-Reply-To` headers for a set of
/// messages, row-aligned, so the conversation view can build a real reply tree.
/// `(None, None)` for a message row with no stored headers.
fn fetch_threading_bulk(
    connection: &Connection,
    rows: &[MessageSummaryRow],
) -> Result<Vec<ThreadingHeaders>, StoreError> {
    const CHUNK_SIZE: usize = 300;

    let mut threading = vec![(None, None); rows.len()];
    for (chunk_offset, chunk) in rows.chunks(CHUNK_SIZE).enumerate() {
        let start_index = chunk_offset * CHUNK_SIZE;
        let mut params = Vec::with_capacity(chunk.len() * 3);
        let mut values = Vec::with_capacity(chunk.len());
        for (offset, row) in chunk.iter().enumerate() {
            values.push("(?, ?, ?)".to_string());
            params.push(SqlValue::Integer((start_index + offset) as i64));
            params.push(SqlValue::Text(row.source_id.as_str().to_string()));
            params.push(SqlValue::Text(row.id.as_str().to_string()));
        }
        let sql = format!(
            "WITH requested(row_index, account_id, message_id) AS (VALUES {})
             SELECT requested.row_index, m.rfc_message_id, m.in_reply_to
               FROM requested
               JOIN message m
                 ON m.account_id = requested.account_id
                AND m.id = requested.message_id",
            values.join(", ")
        );
        let mut statement = connection
            .prepare_cached(&sql)
            .map_err(sql_to_store_error)?;
        let fetched = statement
            .query_map(params_from_iter(params), |row| {
                Ok((
                    row.get::<_, i64>(0)? as usize,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(sql_to_store_error)?;
        for entry in fetched {
            let (row_index, rfc, reply) = entry.map_err(sql_to_store_error)?;
            threading[row_index] = (rfc, reply);
        }
    }

    Ok(threading)
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
        to: parse_recipients_json(row.get(8)?)?,
        preview: row.get(9)?,
        received_at: row.get(10)?,
        has_attachment: row.get::<_, i64>(11)? != 0,
        is_read: row.get::<_, i64>(12)? != 0,
        is_flagged: row.get::<_, i64>(13)? != 0,
    })
}

pub(super) fn parse_recipients_json(value: String) -> Result<Vec<Recipient>, rusqlite::Error> {
    serde_json::from_str(&value)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(8, Type::Text, Box::new(error)))
}

/// Bulk-fetches mailbox IDs for a set of messages in chunks.
fn fetch_mailbox_ids_bulk(
    connection: &Connection,
    rows: &[MessageSummaryRow],
) -> Result<Vec<Vec<MailboxId>>, StoreError> {
    fetch_message_values_bulk(connection, rows, "message_mailbox", "mailbox_id", |row| {
        Ok(MailboxId(row.get(1)?))
    })
}

/// Bulk-fetches keywords for a set of messages in chunks.
fn fetch_keywords_bulk(
    connection: &Connection,
    rows: &[MessageSummaryRow],
) -> Result<Vec<Vec<String>>, StoreError> {
    fetch_message_values_bulk(connection, rows, "message_keyword", "keyword", |row| {
        row.get(1)
    })
}

/// Generic bulk-fetch for message-associated values (mailbox IDs or keywords).
/// Queries in chunks using a VALUES CTE that carries the caller's row index, so
/// hydration can fill row-aligned vectors without cloning `(account_id,
/// message_id)` keys out of SQLite into a HashMap.
fn fetch_message_values_bulk<T>(
    connection: &Connection,
    rows: &[MessageSummaryRow],
    table: &str,
    value_column: &str,
    mut map_value: impl FnMut(&rusqlite::Row<'_>) -> Result<T, rusqlite::Error>,
) -> Result<Vec<Vec<T>>, StoreError> {
    const CHUNK_SIZE: usize = 300;

    let mut values_by_row = (0..rows.len()).map(|_| Vec::new()).collect::<Vec<_>>();
    for (chunk_offset, chunk) in rows.chunks(CHUNK_SIZE).enumerate() {
        let start_index = chunk_offset * CHUNK_SIZE;
        let mut params = Vec::with_capacity(chunk.len() * 3);
        let mut values = Vec::with_capacity(chunk.len());
        for (offset, row) in chunk.iter().enumerate() {
            values.push("(?, ?, ?)".to_string());
            params.push(SqlValue::Integer((start_index + offset) as i64));
            params.push(SqlValue::Text(row.source_id.as_str().to_string()));
            params.push(SqlValue::Text(row.id.as_str().to_string()));
        }
        let sql = format!(
            "WITH requested(row_index, account_id, message_id) AS (VALUES {})
             SELECT requested.row_index, value.{value_column}
               FROM requested
               JOIN {table} value
                 ON value.account_id = requested.account_id
                AND value.message_id = requested.message_id
              ORDER BY requested.row_index, value.{value_column}",
            values.join(", ")
        );
        let mut statement = connection
            .prepare_cached(&sql)
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params_from_iter(params), |row| {
                Ok((row.get::<_, i64>(0)? as usize, map_value(row)?))
            })
            .map_err(sql_to_store_error)?;
        for row in rows {
            let (row_index, value) = row.map_err(sql_to_store_error)?;
            values_by_row[row_index].push(value);
        }
    }

    Ok(values_by_row)
}
