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
                to: row.to,
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
