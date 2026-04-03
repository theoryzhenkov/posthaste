use super::*;

/// Returns (unread, total) counts for messages matching a smart mailbox rule.
///
/// @spec spec/L1-search#smart-mailbox-data-model
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

pub(crate) fn query_conversations_by_rule(
    connection: &Connection,
    rule: &SmartMailboxRule,
    limit: usize,
    cursor: Option<&ConversationCursor>,
) -> Result<ConversationPage, StoreError> {
    let mut params = Vec::new();
    let where_clause = compile_smart_mailbox_rule(rule, &mut params)?;
    query_conversations(
        connection,
        &format!("WHERE ({where_clause})"),
        params,
        limit,
        cursor,
    )
}

pub(crate) fn query_conversations(
    connection: &Connection,
    where_clause: &str,
    mut params: Vec<SqlValue>,
    limit: usize,
    cursor: Option<&ConversationCursor>,
) -> Result<ConversationPage, StoreError> {
    let page_limit = limit.max(1);
    let page_filter = if let Some(cursor) = cursor {
        params.push(SqlValue::Text(cursor.latest_received_at.clone()));
        params.push(SqlValue::Text(cursor.latest_received_at.clone()));
        params.push(SqlValue::Text(cursor.conversation_id.as_str().to_string()));
        "WHERE latest_received_at < ?
           OR (latest_received_at = ? AND conversation_id < ?)"
    } else {
        ""
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
                ranked.is_flagged
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
            latest.is_flagged
        FROM latest
        {page_filter}
        ORDER BY latest.latest_received_at DESC, latest.conversation_id DESC
        LIMIT ?"
    );
    let mut statement = connection.prepare(&sql).map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params_from_iter(params), |row| {
            Ok(ConversationSummary {
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
                latest_message: mail_domain::SourceMessageRef {
                    source_id: AccountId(row.get(10)?),
                    message_id: MessageId(row.get(12)?),
                },
                latest_source_name: row.get(11)?,
                has_attachment: row.get::<_, i64>(13)? != 0,
                is_flagged: row.get::<_, i64>(14)? != 0,
            })
        })
        .map_err(sql_to_store_error)?;
    let mut items = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;
    let has_more = items.len() > page_limit;
    if has_more {
        items.truncate(page_limit);
    }
    let next_cursor = if has_more {
        items.last().map(|item| ConversationCursor {
            latest_received_at: item.latest_received_at.clone(),
            conversation_id: item.id.clone(),
        })
    } else {
        None
    };
    Ok(ConversationPage { items, next_cursor })
}

fn expect_string_value(value: &SmartMailboxValue) -> Result<&str, StoreError> {
    match value {
        SmartMailboxValue::String(value) => Ok(value.as_str()),
        _ => Err(StoreError::Failure(
            "expected string smart mailbox value".to_string(),
        )),
    }
}

fn expect_strings_value(value: &SmartMailboxValue) -> Result<&[String], StoreError> {
    match value {
        SmartMailboxValue::Strings(values) => Ok(values.as_slice()),
        _ => Err(StoreError::Failure(
            "expected string array smart mailbox value".to_string(),
        )),
    }
}

fn expect_bool_value(value: &SmartMailboxValue) -> Result<bool, StoreError> {
    match value {
        SmartMailboxValue::Bool(value) => Ok(*value),
        _ => Err(StoreError::Failure(
            "expected boolean smart mailbox value".to_string(),
        )),
    }
}

fn compile_smart_mailbox_rule(
    rule: &SmartMailboxRule,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    compile_smart_mailbox_group(&rule.root, params)
}

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

fn compile_smart_mailbox_condition(
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    let fragment = match condition.field {
        SmartMailboxField::SourceId => compile_simple_field("m.account_id", condition, params)?,
        SmartMailboxField::SourceName => compile_simple_field("a.name", condition, params)?,
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

fn push_placeholders(values: &[String], params: &mut Vec<SqlValue>) -> String {
    for value in values {
        params.push(SqlValue::Text(value.clone()));
    }
    vec!["?"; values.len()].join(", ")
}

fn split_group_concat_ids(value: Option<String>) -> Vec<AccountId> {
    split_group_concat_strings(value)
        .into_iter()
        .map(AccountId)
        .collect()
}

fn split_group_concat_strings(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split('\u{1f}')
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}
