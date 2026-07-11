use super::*;

/// Returns (unread, total) counts for messages matching a smart mailbox rule.
///
/// @spec docs/L1-search#smart-mailbox-data-model
pub(crate) fn count_smart_mailbox_messages(
    connection: &Connection,
    rule: &MailQueryRule,
) -> Result<(i64, i64), StoreError> {
    let mut params = Vec::new();
    let where_clause = compile_mail_query_rule(rule, &mut params)?;
    let sql = format!(
        "SELECT COUNT(*), SUM(CASE WHEN m.is_read = 0 THEN 1 ELSE 0 END)
         FROM message_effective m
         LEFT JOIN source_projection a ON a.source_id = m.account_id
         WHERE ({where_clause})"
    );
    connection
        .query_row(&sql, params_from_iter(params), |row| {
            let total: i64 = row.get(0)?;
            let unread: i64 = row.get::<_, Option<i64>>(1)?.unwrap_or(0);
            Ok((unread, total))
        })
        .map_err(sql_to_store_error)
}

/// Queries messages matching a smart mailbox rule across all sources, ordered
/// by `received_at DESC`.
///
/// @spec docs/L1-search#smart-mailbox-data-model
pub(crate) fn query_messages_by_rule(
    connection: &Connection,
    rule: &MailQueryRule,
) -> Result<Vec<MessageSummary>, StoreError> {
    let mut params = Vec::new();
    let where_clause = compile_mail_query_rule(rule, &mut params)?;
    let sql = format!(
        "SELECT m.id, m.account_id, COALESCE(a.name, m.account_id), m.thread_id, m.conversation_id, m.subject,
                m.from_name, m.from_email, m.to_json, m.preview, m.received_at, m.has_attachment,
                m.is_read, m.is_flagged
         FROM message_effective m
         LEFT JOIN source_projection a ON a.source_id = m.account_id
         WHERE ({where_clause})
         ORDER BY m.received_at DESC"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(sql_to_store_error)?;
    let rows = load_message_summary_rows(&mut statement, params_from_iter(params))?;
    hydrate_message_summaries(connection, rows)
}

/// Queries messages for any SQL filter with seek pagination.
///
/// @spec docs/L1-api#cursor-pagination
pub(crate) fn query_message_page(
    connection: &Connection,
    where_clause: &str,
    mut params: Vec<SqlValue>,
    limit: usize,
    cursor: Option<&MessageCursor>,
    sort_field: MessageSortField,
    sort_direction: SortDirection,
) -> Result<MessagePage, StoreError> {
    let page_limit = limit.max(1);
    let seek_op = match sort_direction {
        SortDirection::Desc => "<",
        SortDirection::Asc => ">",
    };
    let dir = match sort_direction {
        SortDirection::Desc => "DESC",
        SortDirection::Asc => "ASC",
    };
    let sort_key = message_sort_key_expr(sort_field);
    let page_filter = if let Some(cursor) = cursor {
        params.push(message_cursor_sort_sql_value(
            sort_field,
            &cursor.sort_value,
        )?);
        params.push(message_cursor_sort_sql_value(
            sort_field,
            &cursor.sort_value,
        )?);
        params.push(SqlValue::Text(message_cursor_tie_key(cursor)));
        format!("WHERE sort_key {seek_op} ? OR (sort_key = ? AND tie_key {seek_op} ?)")
    } else {
        String::new()
    };
    params.push(SqlValue::Integer((page_limit + 1) as i64));
    let sql = format!(
        "WITH filtered AS (
            SELECT
                m.id,
                m.account_id,
                COALESCE(a.name, m.account_id) AS name,
                m.thread_id,
                m.conversation_id,
                m.subject,
                m.from_name,
                m.from_email,
                m.to_json,
                m.preview,
                m.received_at,
                m.has_attachment,
                m.is_read,
                m.is_flagged,
                {sort_key} AS sort_key,
                m.account_id || char(31) || m.id AS tie_key
            FROM message_effective m
            LEFT JOIN source_projection a
              ON a.source_id = m.account_id
            {where_clause}
        )
        SELECT
            id,
            account_id,
            name,
            thread_id,
            conversation_id,
            subject,
            from_name,
            from_email,
            to_json,
            preview,
            received_at,
            has_attachment,
            is_read,
            is_flagged,
            sort_key
        FROM filtered
        {page_filter}
        ORDER BY sort_key {dir}, tie_key {dir}
        LIMIT ?"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params_from_iter(params), |row| {
            let summary = row_to_message_summary_row(row)?;
            let sort_key_value: rusqlite::types::Value = row.get(14)?;
            Ok((summary, sort_key_value))
        })
        .map_err(sql_to_store_error)?;
    let mut rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;
    let has_more = rows.len() > page_limit;
    if has_more {
        rows.truncate(page_limit);
    }
    let next_cursor = if has_more {
        rows.last().map(|(row, sort_key_value)| MessageCursor {
            sort_value: sql_value_to_cursor_string(sort_key_value),
            source_id: row.source_id.clone(),
            message_id: row.id.clone(),
        })
    } else {
        None
    };
    let rows = rows.into_iter().map(|(row, _)| row).collect();
    Ok(MessagePage {
        items: hydrate_message_summaries(connection, rows)?,
        next_cursor,
    })
}

/// Queries all messages matching a smart mailbox rule with explicit ordering.
pub(crate) fn query_messages_by_rule_sorted(
    connection: &Connection,
    rule: &MailQueryRule,
    sort_field: MessageSortField,
    sort_direction: SortDirection,
) -> Result<Vec<MessageSummary>, StoreError> {
    let mut params = Vec::new();
    let where_clause = compile_mail_query_rule(rule, &mut params)?;
    let sort_key = message_sort_key_expr(sort_field);
    let dir = match sort_direction {
        SortDirection::Desc => "DESC",
        SortDirection::Asc => "ASC",
    };
    let sql = format!(
        "SELECT m.id, m.account_id, COALESCE(a.name, m.account_id), m.thread_id, m.conversation_id, m.subject,
                m.from_name, m.from_email, m.to_json, m.preview, m.received_at, m.has_attachment,
                m.is_read, m.is_flagged
         FROM message_effective m
         LEFT JOIN source_projection a ON a.source_id = m.account_id
         WHERE ({where_clause})
         ORDER BY {sort_key} {dir}, m.account_id {dir}, m.id {dir}"
    );
    let mut statement = connection
        .prepare_cached(&sql)
        .map_err(sql_to_store_error)?;
    let rows = load_message_summary_rows(&mut statement, params_from_iter(params))?;
    hydrate_message_summaries(connection, rows)
}

/// Queries messages matching a smart mailbox rule with seek pagination.
///
/// @spec docs/L1-api#cursor-pagination
pub(crate) fn query_message_page_by_rule(
    connection: &Connection,
    rule: &MailQueryRule,
    limit: usize,
    cursor: Option<&MessageCursor>,
    sort_field: MessageSortField,
    sort_direction: SortDirection,
) -> Result<MessagePage, StoreError> {
    let mut params = Vec::new();
    let where_clause = compile_mail_query_rule(rule, &mut params)?;
    query_message_page(
        connection,
        &format!("WHERE ({where_clause})"),
        params,
        limit,
        cursor,
        sort_field,
        sort_direction,
    )
}

fn message_sort_key_expr(sort_field: MessageSortField) -> &'static str {
    match sort_field {
        MessageSortField::Date => "m.received_at",
        MessageSortField::From => "LOWER(COALESCE(m.from_name, m.from_email, ''))",
        MessageSortField::Subject => "LOWER(COALESCE(m.subject, ''))",
        MessageSortField::Source => "LOWER(COALESCE(a.name, m.account_id))",
        MessageSortField::Flagged => "m.is_flagged",
        MessageSortField::Attachment => "m.has_attachment",
    }
}

fn is_numeric_message_sort(sort_field: MessageSortField) -> bool {
    matches!(
        sort_field,
        MessageSortField::Flagged | MessageSortField::Attachment
    )
}

fn message_cursor_sort_sql_value(
    sort_field: MessageSortField,
    raw: &str,
) -> Result<SqlValue, StoreError> {
    if is_numeric_message_sort(sort_field) {
        let n = raw
            .parse::<i64>()
            .map_err(|_| StoreError::Failure(format!("invalid numeric cursor value: {raw}")))?;
        Ok(SqlValue::Integer(n))
    } else {
        Ok(SqlValue::Text(raw.to_string()))
    }
}

fn message_cursor_tie_key(cursor: &MessageCursor) -> String {
    format!(
        "{}\u{1f}{}",
        cursor.source_id.as_str(),
        cursor.message_id.as_str()
    )
}

fn sql_value_to_cursor_string(value: &rusqlite::types::Value) -> String {
    match value {
        rusqlite::types::Value::Integer(n) => n.to_string(),
        rusqlite::types::Value::Text(s) => s.clone(),
        rusqlite::types::Value::Real(f) => f.to_string(),
        _ => String::new(),
    }
}
