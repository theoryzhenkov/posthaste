use super::*;

/// Returns (unread, total) counts for messages matching a smart mailbox rule.
///
/// @spec docs/L1-search#smart-mailbox-data-model
pub(crate) fn count_smart_mailbox_messages(
    connection: &Connection,
    rule: &SmartMailboxRule,
) -> Result<(i64, i64), StoreError> {
    let mut params = Vec::new();
    let where_clause = compile_smart_mailbox_rule(rule, &mut params)?;
    let sql = format!(
        "SELECT COUNT(*), SUM(CASE WHEN m.is_read = 0 THEN 1 ELSE 0 END)
         FROM message m
         JOIN source_projection a ON a.source_id = m.account_id
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
    rule: &SmartMailboxRule,
) -> Result<Vec<MessageSummary>, StoreError> {
    let mut params = Vec::new();
    let where_clause = compile_smart_mailbox_rule(rule, &mut params)?;
    let sql = format!(
        "SELECT m.id, m.account_id, a.name, m.thread_id, m.conversation_id, m.subject,
                m.from_name, m.from_email, m.preview, m.received_at, m.has_attachment,
                m.is_read, m.is_flagged
         FROM message m
         JOIN source_projection a ON a.source_id = m.account_id
         WHERE ({where_clause})
         ORDER BY m.received_at DESC"
    );
    let mut statement = connection.prepare(&sql).map_err(sql_to_store_error)?;
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
                a.name,
                m.thread_id,
                m.conversation_id,
                m.subject,
                m.from_name,
                m.from_email,
                m.preview,
                m.received_at,
                m.has_attachment,
                m.is_read,
                m.is_flagged,
                {sort_key} AS sort_key,
                m.account_id || char(31) || m.id AS tie_key
            FROM message m
            JOIN source_projection a
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
    let mut statement = connection.prepare(&sql).map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params_from_iter(params), |row| {
            let summary = row_to_message_summary_row(row)?;
            let sort_key_value: rusqlite::types::Value = row.get(13)?;
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

/// Queries messages matching a smart mailbox rule with seek pagination.
///
/// @spec docs/L1-api#cursor-pagination
pub(crate) fn query_message_page_by_rule(
    connection: &Connection,
    rule: &SmartMailboxRule,
    limit: usize,
    cursor: Option<&MessageCursor>,
    sort_field: MessageSortField,
    sort_direction: SortDirection,
) -> Result<MessagePage, StoreError> {
    let mut params = Vec::new();
    let where_clause = compile_smart_mailbox_rule(rule, &mut params)?;
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

fn message_sort_key_expr(sort_field: MessageSortField) -> &'static str {
    match sort_field {
        MessageSortField::Date => "m.received_at",
        MessageSortField::From => "LOWER(COALESCE(m.from_name, m.from_email, ''))",
        MessageSortField::Subject => "LOWER(COALESCE(m.subject, ''))",
        MessageSortField::Source => "LOWER(a.name)",
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

/// Whether the sort field stores an integer value (affects cursor binding type).
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

/// Extracts a string value or returns a type error.
fn expect_string_value(value: &SmartMailboxValue) -> Result<&str, StoreError> {
    match value {
        SmartMailboxValue::String(value) => Ok(value.as_str()),
        _ => Err(StoreError::Failure(
            "expected string smart mailbox value".to_string(),
        )),
    }
}

/// Extracts a string array value or returns a type error.
fn expect_strings_value(value: &SmartMailboxValue) -> Result<&[String], StoreError> {
    match value {
        SmartMailboxValue::Strings(values) => Ok(values.as_slice()),
        _ => Err(StoreError::Failure(
            "expected string array smart mailbox value".to_string(),
        )),
    }
}

/// Extracts a boolean value or returns a type error.
fn expect_bool_value(value: &SmartMailboxValue) -> Result<bool, StoreError> {
    match value {
        SmartMailboxValue::Bool(value) => Ok(*value),
        _ => Err(StoreError::Failure(
            "expected boolean smart mailbox value".to_string(),
        )),
    }
}

/// Compiles a smart mailbox rule tree into a SQL WHERE clause with
/// parameterized bindings.
fn compile_smart_mailbox_rule(
    rule: &SmartMailboxRule,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    compile_smart_mailbox_group(&rule.root, params)
}

/// Recursively compiles a rule group into SQL, joining nodes with AND/OR and
/// optionally wrapping in NOT.
fn compile_smart_mailbox_group(
    group: &SmartMailboxGroup,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    if group.nodes.is_empty() {
        return Ok(if group.negated {
            "NOT (1 = 1)".to_string()
        } else {
            "1 = 1".to_string()
        });
    }
    let joiner = match group.operator {
        SmartMailboxGroupOperator::All => " AND ",
        SmartMailboxGroupOperator::Any => " OR ",
    };
    let mut parts = Vec::with_capacity(group.nodes.len());
    for node in &group.nodes {
        let fragment = match node {
            SmartMailboxRuleNode::Group(group) => compile_smart_mailbox_group(group, params)?,
            SmartMailboxRuleNode::Condition(condition) => {
                compile_smart_mailbox_condition(condition, params)?
            }
        };
        parts.push(format!("({fragment})"));
    }
    let combined = parts.join(joiner);
    Ok(if group.negated {
        format!("NOT ({combined})")
    } else {
        combined
    })
}

/// Compiles a single condition into a SQL fragment, dispatching to
/// field-specific compilers.
fn compile_smart_mailbox_condition(
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    let fragment = match condition.field {
        SmartMailboxField::SourceId => compile_simple_field("m.account_id", condition, params)?,
        SmartMailboxField::SourceName => compile_text_field("a.name", condition, params)?,
        SmartMailboxField::MessageId => compile_simple_field("m.id", condition, params)?,
        SmartMailboxField::ThreadId => compile_simple_field("m.thread_id", condition, params)?,
        SmartMailboxField::FromName => compile_text_field("m.from_name", condition, params)?,
        SmartMailboxField::FromEmail => compile_text_field("m.from_email", condition, params)?,
        SmartMailboxField::Subject => compile_text_field("m.subject", condition, params)?,
        SmartMailboxField::Preview => compile_text_field("m.preview", condition, params)?,
        SmartMailboxField::ReceivedAt => compile_date_field("m.received_at", condition, params)?,
        SmartMailboxField::IsRead => compile_bool_field("m.is_read", condition)?,
        SmartMailboxField::IsFlagged => compile_bool_field("m.is_flagged", condition)?,
        SmartMailboxField::HasAttachment => compile_bool_field("m.has_attachment", condition)?,
        SmartMailboxField::MailboxId => compile_exists_membership(
            "EXISTS (
                SELECT 1
                FROM message_mailbox mm
                WHERE mm.account_id = m.account_id
                  AND mm.message_id = m.id
                  AND mm.mailbox_id",
            condition,
            params,
        )?,
        SmartMailboxField::MailboxName => compile_exists_text_membership(
            "EXISTS (
                SELECT 1
                FROM message_mailbox mm
                JOIN mailbox b
                  ON b.account_id = mm.account_id
                 AND b.id = mm.mailbox_id
                WHERE mm.account_id = m.account_id
                  AND mm.message_id = m.id
                  AND b.name",
            condition,
            params,
        )?,
        SmartMailboxField::Keyword => compile_exists_membership(
            "EXISTS (
                SELECT 1
                FROM message_keyword mk
                WHERE mk.account_id = m.account_id
                  AND mk.message_id = m.id
                  AND mk.keyword",
            condition,
            params,
        )?,
        SmartMailboxField::MailboxRole => compile_exists_membership(
            "EXISTS (
                SELECT 1
                FROM message_mailbox mm
                JOIN mailbox b
                  ON b.account_id = mm.account_id
                 AND b.id = mm.mailbox_id
                WHERE mm.account_id = m.account_id
                  AND mm.message_id = m.id
                  AND b.role",
            condition,
            params,
        )?,
    };
    Ok(if condition.negated {
        format!("NOT ({fragment})")
    } else {
        fragment
    })
}

/// Compiles an `equals` or `in` condition against a simple column.
fn compile_simple_field(
    column: &str,
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    match condition.operator {
        SmartMailboxOperator::Equals => {
            params.push(SqlValue::Text(
                expect_string_value(&condition.value)?.to_string(),
            ));
            Ok(format!("{column} = ?"))
        }
        SmartMailboxOperator::In => {
            let values = expect_strings_value(&condition.value)?;
            compile_in_clause(column, values, params)
        }
        _ => Err(StoreError::Failure(format!(
            "unsupported operator {:?} for field {:?}",
            condition.operator, condition.field
        ))),
    }
}

/// Compiles a text field condition, handling NULL with COALESCE and
/// case-insensitive `contains` via LOWER/LIKE.
fn compile_text_field(
    column: &str,
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    match condition.operator {
        SmartMailboxOperator::Equals => {
            params.push(SqlValue::Text(
                expect_string_value(&condition.value)?.to_string(),
            ));
            Ok(format!("COALESCE({column}, '') = ?"))
        }
        SmartMailboxOperator::Contains => {
            params.push(SqlValue::Text(format!(
                "%{}%",
                expect_string_value(&condition.value)?.to_lowercase()
            )));
            Ok(format!("LOWER(COALESCE({column}, '')) LIKE ?"))
        }
        SmartMailboxOperator::In => {
            let values = expect_strings_value(&condition.value)?;
            compile_in_clause(&format!("COALESCE({column}, '')"), values, params)
        }
        _ => Err(StoreError::Failure(format!(
            "unsupported operator {:?} for field {:?}",
            condition.operator, condition.field
        ))),
    }
}

/// Compiles a date comparison condition (before/after/on-or-before/on-or-after).
fn compile_date_field(
    column: &str,
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    params.push(SqlValue::Text(
        expect_string_value(&condition.value)?.to_string(),
    ));
    let comparator = match condition.operator {
        SmartMailboxOperator::Before => "<",
        SmartMailboxOperator::After => ">",
        SmartMailboxOperator::OnOrBefore => "<=",
        SmartMailboxOperator::OnOrAfter => ">=",
        _ => {
            return Err(StoreError::Failure(format!(
                "unsupported operator {:?} for field {:?}",
                condition.operator, condition.field
            )))
        }
    };
    Ok(format!("{column} {comparator} ?"))
}

/// Compiles a boolean field equality check (integer 0/1).
fn compile_bool_field(
    column: &str,
    condition: &SmartMailboxCondition,
) -> Result<String, StoreError> {
    if !matches!(condition.operator, SmartMailboxOperator::Equals) {
        return Err(StoreError::Failure(format!(
            "unsupported operator {:?} for field {:?}",
            condition.operator, condition.field
        )));
    }
    let expected = if expect_bool_value(&condition.value)? {
        1
    } else {
        0
    };
    Ok(format!("{column} = {expected}"))
}

/// Compiles a condition that checks membership via an EXISTS subquery
/// (mailbox ID, keyword, or mailbox role).
fn compile_exists_membership(
    prefix: &str,
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    let suffix = match condition.operator {
        SmartMailboxOperator::Equals => {
            params.push(SqlValue::Text(
                expect_string_value(&condition.value)?.to_string(),
            ));
            " = ?".to_string()
        }
        SmartMailboxOperator::In => {
            let values = expect_strings_value(&condition.value)?;
            let placeholders = push_placeholders(values, params);
            format!(" IN ({placeholders})")
        }
        _ => {
            return Err(StoreError::Failure(format!(
                "unsupported operator {:?} for field {:?}",
                condition.operator, condition.field
            )))
        }
    };
    Ok(format!("{prefix}{suffix}\n            )"))
}

/// Compiles text membership via an EXISTS subquery, currently used for mailbox
/// display names.
fn compile_exists_text_membership(
    prefix: &str,
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    let suffix = match condition.operator {
        SmartMailboxOperator::Equals => {
            params.push(SqlValue::Text(
                expect_string_value(&condition.value)?.to_string(),
            ));
            " = ?".to_string()
        }
        SmartMailboxOperator::Contains => {
            params.push(SqlValue::Text(format!(
                "%{}%",
                expect_string_value(&condition.value)?.to_lowercase()
            )));
            " IS NOT NULL
                  AND LOWER(b.name) LIKE ?"
                .to_string()
        }
        SmartMailboxOperator::In => {
            let values = expect_strings_value(&condition.value)?;
            let placeholders = push_placeholders(values, params);
            format!(" IN ({placeholders})")
        }
        _ => {
            return Err(StoreError::Failure(format!(
                "unsupported operator {:?} for field {:?}",
                condition.operator, condition.field
            )))
        }
    };
    Ok(format!("{prefix}{suffix}\n            )"))
}

/// Builds a SQL `IN (?, ?, ...)` clause, returning `1 = 0` for empty lists.
fn compile_in_clause(
    column: &str,
    values: &[String],
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    if values.is_empty() {
        return Ok("1 = 0".to_string());
    }
    let placeholders = push_placeholders(values, params);
    Ok(format!("{column} IN ({placeholders})"))
}

/// Pushes string values onto the params list and returns comma-separated `?`
/// placeholders.
fn push_placeholders(values: &[String], params: &mut Vec<SqlValue>) -> String {
    for value in values {
        params.push(SqlValue::Text(value.clone()));
    }
    vec!["?"; values.len()].join(", ")
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
