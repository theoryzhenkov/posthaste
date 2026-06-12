use super::*;

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

/// Compiles an `equals` or `in` condition against a simple column.
pub(super) fn compile_simple_field(
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
pub(super) fn compile_text_field(
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
pub(super) fn compile_date_field(
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
pub(super) fn compile_bool_field(
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
pub(super) fn compile_exists_membership(
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
pub(super) fn compile_exists_text_membership(
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
