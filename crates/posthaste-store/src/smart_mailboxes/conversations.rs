use super::*;

/// Queries conversations matching a smart mailbox rule with seek pagination.
///
/// @spec docs/L1-sync#conversation-pagination
pub(crate) fn query_conversations_by_rule(
    connection: &Connection,
    rule: &SmartMailboxRule,
    limit: usize,
    cursor: Option<&ConversationCursor>,
    sort_field: ConversationSortField,
    sort_direction: SortDirection,
) -> Result<ConversationPage, StoreError> {
    let mut params = Vec::new();
    let where_clause = compile_smart_mailbox_rule(rule, &mut params)?;
    query_conversations(
        connection,
        &format!("WHERE ({where_clause})"),
        params,
        limit,
        cursor,
        sort_field,
        sort_direction,
    )
}

/// SQL expression for the sort key computed in the `latest` CTE.
///
/// Uses the `ranked.` prefix for column references because the expression is
/// evaluated inside the `latest` CTE SELECT (not the final SELECT), so it must
/// reference `ranked` columns directly rather than aliases defined in the same
/// SELECT clause.
fn sort_key_expr(sort_field: ConversationSortField) -> &'static str {
    match sort_field {
        ConversationSortField::Date => "ranked.received_at",
        ConversationSortField::From => "LOWER(COALESCE(ranked.from_name, ranked.from_email, ''))",
        ConversationSortField::Subject => "LOWER(COALESCE(ranked.subject, ''))",
        ConversationSortField::Source => "LOWER(ranked.account_name)",
        ConversationSortField::ThreadSize => "ranked.message_count",
        ConversationSortField::Flagged => "ranked.is_flagged",
        ConversationSortField::Attachment => "ranked.has_attachment",
    }
}

fn is_numeric_sort(sort_field: ConversationSortField) -> bool {
    matches!(
        sort_field,
        ConversationSortField::ThreadSize
            | ConversationSortField::Flagged
            | ConversationSortField::Attachment
    )
}

/// Bind a cursor's sort_value as the correct SQL type for the sort field.
fn cursor_sort_sql_value(
    sort_field: ConversationSortField,
    raw: &str,
) -> Result<SqlValue, StoreError> {
    if is_numeric_sort(sort_field) {
        let n = raw
            .parse::<i64>()
            .map_err(|_| StoreError::Failure(format!("invalid numeric cursor value: {raw}")))?;
        Ok(SqlValue::Integer(n))
    } else {
        Ok(SqlValue::Text(raw.to_string()))
    }
}

/// Core conversation pagination query using CTEs: filters messages, ranks by
/// recency within each conversation, groups sources, and applies seek-based
/// cursor pagination with configurable sort field and direction.
///
/// @spec docs/L1-sync#conversation-pagination
pub(crate) fn query_conversations(
    connection: &Connection,
    where_clause: &str,
    mut params: Vec<SqlValue>,
    limit: usize,
    cursor: Option<&ConversationCursor>,
    sort_field: ConversationSortField,
    sort_direction: SortDirection,
) -> Result<ConversationPage, StoreError> {
    let page_limit = limit.max(1);
    let seek_op = match sort_direction {
        SortDirection::Desc => "<",
        SortDirection::Asc => ">",
    };
    let dir = match sort_direction {
        SortDirection::Desc => "DESC",
        SortDirection::Asc => "ASC",
    };
    let sort_key = sort_key_expr(sort_field);
    let page_filter = if let Some(cursor) = cursor {
        params.push(cursor_sort_sql_value(sort_field, &cursor.sort_value)?);
        params.push(cursor_sort_sql_value(sort_field, &cursor.sort_value)?);
        params.push(SqlValue::Text(cursor.conversation_id.as_str().to_string()));
        format!(
            "WHERE sort_key {seek_op} ?
               OR (sort_key = ? AND conversation_id {seek_op} ?)"
        )
    } else {
        String::new()
    };
    params.push(SqlValue::Integer((page_limit + 1) as i64));
    let sql = format!(
        "WITH filtered AS (
            SELECT
                m.conversation_id,
                m.account_id,
                a.name AS account_name,
                m.id,
                m.subject,
                m.from_name,
                m.from_email,
                m.to_json,
                m.preview,
                m.received_at,
                m.has_attachment,
                m.is_read,
                m.is_flagged
            FROM message m
            JOIN source_projection a
              ON a.source_id = m.account_id
            {where_clause}
        ),
        ranked AS (
            SELECT
                filtered.*,
                ROW_NUMBER() OVER (
                    PARTITION BY filtered.conversation_id
                    ORDER BY filtered.received_at DESC, filtered.id DESC
                ) AS row_number,
                COUNT(*) OVER (PARTITION BY filtered.conversation_id) AS message_count,
                SUM(CASE WHEN filtered.is_read = 0 THEN 1 ELSE 0 END)
                    OVER (PARTITION BY filtered.conversation_id) AS unread_count
            FROM filtered
        ),
        distinct_source_groups AS (
            SELECT DISTINCT
                filtered.conversation_id,
                filtered.account_id,
                filtered.account_name
            FROM filtered
        ),
        source_groups AS (
            SELECT
                distinct_source_groups.conversation_id,
                GROUP_CONCAT(distinct_source_groups.account_id, char(31)) AS source_ids,
                GROUP_CONCAT(distinct_source_groups.account_name, char(31)) AS source_names
            FROM distinct_source_groups
            GROUP BY distinct_source_groups.conversation_id
        ),
        latest AS (
            SELECT
                ranked.conversation_id,
                ranked.subject,
                ranked.preview,
                ranked.from_name,
                ranked.from_email,
                ranked.received_at AS latest_received_at,
                ranked.unread_count,
                ranked.message_count,
                source_groups.source_ids,
                source_groups.source_names,
                ranked.account_id,
                ranked.account_name,
                ranked.id,
                ranked.has_attachment,
                ranked.is_flagged,
                {sort_key} AS sort_key
            FROM ranked
            JOIN source_groups
              ON source_groups.conversation_id = ranked.conversation_id
            WHERE ranked.row_number = 1
        )
        SELECT
            latest.conversation_id,
            latest.subject,
            latest.preview,
            latest.from_name,
            latest.from_email,
            latest.latest_received_at,
            latest.unread_count,
            latest.message_count,
            latest.source_ids,
            latest.source_names,
            latest.account_id,
            latest.account_name,
            latest.id,
            latest.has_attachment,
            latest.is_flagged,
            latest.sort_key
        FROM latest
        {page_filter}
        ORDER BY latest.sort_key {dir}, latest.conversation_id {dir}
        LIMIT ?"
    );
    let mut statement = connection.prepare(&sql).map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params_from_iter(params), |row| {
            let sort_key_value: rusqlite::types::Value = row.get(15)?;
            Ok((
                ConversationSummary {
                    id: ConversationId(row.get(0)?),
                    subject: row.get(1)?,
                    preview: row.get(2)?,
                    from_name: row.get(3)?,
                    from_email: row.get(4)?,
                    latest_received_at: row.get(5)?,
                    unread_count: row.get(6)?,
                    message_count: row.get(7)?,
                    source_ids: split_group_concat_ids(row.get::<_, Option<String>>(8)?),
                    source_names: split_group_concat_strings(row.get::<_, Option<String>>(9)?),
                    latest_message: posthaste_domain::SourceMessageRef {
                        source_id: AccountId(row.get(10)?),
                        message_id: MessageId(row.get(12)?),
                    },
                    latest_source_name: row.get(11)?,
                    has_attachment: row.get::<_, i64>(13)? != 0,
                    is_flagged: row.get::<_, i64>(14)? != 0,
                },
                sort_key_value,
            ))
        })
        .map_err(sql_to_store_error)?;
    let mut items: Vec<(ConversationSummary, rusqlite::types::Value)> = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;
    let has_more = items.len() > page_limit;
    if has_more {
        items.truncate(page_limit);
    }
    let next_cursor = if has_more {
        items.last().map(|(item, sort_key_val)| {
            let sort_value = match sort_key_val {
                rusqlite::types::Value::Integer(n) => n.to_string(),
                rusqlite::types::Value::Text(s) => s.clone(),
                rusqlite::types::Value::Real(f) => f.to_string(),
                _ => String::new(),
            };
            ConversationCursor {
                sort_value,
                conversation_id: item.id.clone(),
            }
        })
    } else {
        None
    };
    let items = items.into_iter().map(|(summary, _)| summary).collect();
    Ok(ConversationPage { items, next_cursor })
}

/// Splits a GROUP_CONCAT result (unit separator delimited) into `AccountId`s.
fn split_group_concat_ids(value: Option<String>) -> Vec<AccountId> {
    split_group_concat_strings(value)
        .into_iter()
        .map(AccountId)
        .collect()
}

/// Splits a GROUP_CONCAT result (unit separator delimited) into strings.
fn split_group_concat_strings(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split('\u{1f}')
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}
