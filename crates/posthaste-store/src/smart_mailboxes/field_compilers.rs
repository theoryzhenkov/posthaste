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

/// Escapes the LIKE metacharacters (`%`, `_`) — and the escape character itself
/// (`\`) — in a user-supplied value so a `beginsWith`/`endsWith` match treats them
/// literally. The compiled clause pairs this with `ESCAPE '\'`, so e.g. a value of
/// `50%` matches a literal `50%` prefix rather than "50" followed by anything. The
/// value is still bound as a parameter — this only stops it acting as a wildcard,
/// it is never an injection surface.
fn escape_like(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
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
        // Prefix/suffix matches: case-insensitive `LIKE` with the value's LIKE
        // metacharacters escaped (so a literal `%`/`_` is not a wildcard), paired
        // with `ESCAPE '\'`.
        SmartMailboxOperator::BeginsWith => {
            params.push(SqlValue::Text(format!(
                "{}%",
                escape_like(&expect_string_value(&condition.value)?.to_lowercase())
            )));
            Ok(format!("LOWER(COALESCE({column}, '')) LIKE ? ESCAPE '\\'"))
        }
        SmartMailboxOperator::EndsWith => {
            params.push(SqlValue::Text(format!(
                "%{}",
                escape_like(&expect_string_value(&condition.value)?.to_lowercase())
            )));
            Ok(format!("LOWER(COALESCE({column}, '')) LIKE ? ESCAPE '\\'"))
        }
        // Regex: the pattern is bound as a parameter and evaluated by the
        // `regexp` scalar registered on the connection (see `db/connection.rs`).
        // PERF: a regex predicate cannot use an index, so it is a full scan of the
        // candidate set — acceptable for a smart-mailbox/rule filter, but noted.
        // A malformed pattern is rejected at the write boundary (R5c
        // `validate_condition`), so it never reaches here; if one somehow did, the
        // scalar surfaces a `StoreError`, never a panic.
        SmartMailboxOperator::Regex => {
            params.push(SqlValue::Text(
                expect_string_value(&condition.value)?.to_string(),
            ));
            Ok(format!("COALESCE({column}, '') REGEXP ?"))
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
///
/// Accepts three value shapes, back-compatibly:
/// - a legacy bare [`SmartMailboxValue::String`] — an absolute RFC3339 instant
///   compared against the stored `received_at` literally (unchanged behavior);
/// - [`DateValue::Absolute`] — the same literal comparison, but typed;
/// - [`DateValue::Relative`] — a *rolling* bound resolved at query time via
///   SQLite's `datetime('now', ?)`, so "in the last N days" keeps rolling with
///   the clock instead of freezing to a fixed instant at edit time.
pub(super) fn compile_date_field(
    column: &str,
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    let comparator = match condition.operator {
        SmartMailboxOperator::Lt => "<",
        SmartMailboxOperator::Gt => ">",
        SmartMailboxOperator::Le => "<=",
        SmartMailboxOperator::Ge => ">=",
        _ => {
            return Err(StoreError::Failure(format!(
                "unsupported operator {:?} for field {:?}",
                condition.operator, condition.field
            )))
        }
    };
    match &condition.value {
        // Legacy bare-string absolute date, and the typed absolute date, both
        // compare against the stored RFC3339 instant as-is.
        SmartMailboxValue::String(instant) => {
            params.push(SqlValue::Text(instant.clone()));
            Ok(format!("{column} {comparator} ?"))
        }
        SmartMailboxValue::Date(DateValue::Absolute { value }) => {
            params.push(SqlValue::Text(value.clone()));
            Ok(format!("{column} {comparator} ?"))
        }
        // Rolling relative bound: emit `datetime(received_at) <cmp>
        // datetime('now', '-N unit')`. `received_at` is stored as RFC3339 TEXT,
        // so both sides go through `datetime()` for a real instant comparison.
        // The `-N unit` modifier is built from a bound-integer amount (`u32`,
        // digits only) and a fixed, validated unit string, then passed as a
        // *bound parameter* — no user text ever reaches the SQL, so there is no
        // injection surface.
        SmartMailboxValue::Date(DateValue::Relative { amount, unit }) => {
            let modifier = match unit {
                DateUnit::Minutes => format!("-{amount} minutes"),
                DateUnit::Hours => format!("-{amount} hours"),
                DateUnit::Days => format!("-{amount} days"),
                // SQLite has no `weeks` modifier; express it as 7-day multiples.
                DateUnit::Weeks => format!("-{} days", u64::from(*amount) * 7),
                DateUnit::Months => format!("-{amount} months"),
            };
            params.push(SqlValue::Text(modifier));
            Ok(format!(
                "datetime({column}) {comparator} datetime('now', ?)"
            ))
        }
        _ => Err(StoreError::Failure(format!(
            "expected date smart mailbox value for field {:?}",
            condition.field
        ))),
    }
}

/// Compiles a numeric-comparison condition against an integer column (used for
/// `size`). The neutral ordered operators (`Lt/Gt/Le/Ge`) are the numeric
/// `< > <= >=` comparators (mirroring the date compiler's shape, but
/// binding an integer so SQLite compares numerically, not lexicographically).
/// The wire value is a byte count encoded as a string.
pub(super) fn compile_numeric_field(
    column: &str,
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    let comparator = match condition.operator {
        SmartMailboxOperator::Lt => "<",
        SmartMailboxOperator::Gt => ">",
        SmartMailboxOperator::Le => "<=",
        SmartMailboxOperator::Ge => ">=",
        _ => {
            return Err(StoreError::Failure(format!(
                "unsupported operator {:?} for field {:?}",
                condition.operator, condition.field
            )))
        }
    };
    let raw = expect_string_value(&condition.value)?;
    let number = raw.trim().parse::<i64>().map_err(|_| {
        StoreError::Failure(format!(
            "expected integer smart mailbox value for field {:?}, got {raw:?}",
            condition.field
        ))
    })?;
    params.push(SqlValue::Integer(number));
    Ok(format!("{column} {comparator} ?"))
}

/// Compiles an address condition against a JSON recipient column (`to_json`, a
/// JSON array of `{ "name": ..., "email": ... }`). Uses `json_each` so matching
/// is per-recipient and structured rather than a blob `LIKE`:
/// - `Equals` matches a recipient whose email equals the value exactly;
/// - `Contains` matches a substring of either the email or the display name
///   (case-insensitive), mirroring how the `from:` grammar expansion searches
///   both address parts;
/// - `In` matches a recipient whose email is any of the listed values.
pub(super) fn compile_recipient_json_field(
    column: &str,
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    let predicate = match condition.operator {
        SmartMailboxOperator::Equals => {
            params.push(SqlValue::Text(
                expect_string_value(&condition.value)?.to_string(),
            ));
            "json_extract(r.value, '$.email') = ?".to_string()
        }
        SmartMailboxOperator::Contains => {
            let needle = format!(
                "%{}%",
                expect_string_value(&condition.value)?.to_lowercase()
            );
            // Bind twice: once for the email part, once for the display name.
            params.push(SqlValue::Text(needle.clone()));
            params.push(SqlValue::Text(needle));
            "(LOWER(COALESCE(json_extract(r.value, '$.email'), '')) LIKE ?\n                  \
             OR LOWER(COALESCE(json_extract(r.value, '$.name'), '')) LIKE ?)"
                .to_string()
        }
        // Prefix/suffix against either address part (email or display name),
        // case-insensitive with LIKE metacharacters escaped.
        SmartMailboxOperator::BeginsWith | SmartMailboxOperator::EndsWith => {
            let escaped = escape_like(&expect_string_value(&condition.value)?.to_lowercase());
            let needle = if condition.operator == SmartMailboxOperator::BeginsWith {
                format!("{escaped}%")
            } else {
                format!("%{escaped}")
            };
            params.push(SqlValue::Text(needle.clone()));
            params.push(SqlValue::Text(needle));
            "(LOWER(COALESCE(json_extract(r.value, '$.email'), '')) LIKE ? ESCAPE '\\'\n                  \
             OR LOWER(COALESCE(json_extract(r.value, '$.name'), '')) LIKE ? ESCAPE '\\')"
                .to_string()
        }
        // Regex against either address part. Bound twice, once per part; PERF: a
        // full scan (no index), acceptable for a filter.
        SmartMailboxOperator::Regex => {
            let pattern = expect_string_value(&condition.value)?.to_string();
            params.push(SqlValue::Text(pattern.clone()));
            params.push(SqlValue::Text(pattern));
            "(COALESCE(json_extract(r.value, '$.email'), '') REGEXP ?\n                  \
             OR COALESCE(json_extract(r.value, '$.name'), '') REGEXP ?)"
                .to_string()
        }
        SmartMailboxOperator::In => {
            let values = expect_strings_value(&condition.value)?;
            if values.is_empty() {
                return Ok("1 = 0".to_string());
            }
            let placeholders = push_placeholders(values, params);
            format!("json_extract(r.value, '$.email') IN ({placeholders})")
        }
        _ => {
            return Err(StoreError::Failure(format!(
                "unsupported operator {:?} for field {:?}",
                condition.operator, condition.field
            )))
        }
    };
    Ok(format!(
        "EXISTS (\n                SELECT 1\n                FROM json_each({column}) r\n                WHERE {predicate}\n            )"
    ))
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
        SmartMailboxOperator::BeginsWith | SmartMailboxOperator::EndsWith => {
            let escaped = escape_like(&expect_string_value(&condition.value)?.to_lowercase());
            let needle = if condition.operator == SmartMailboxOperator::BeginsWith {
                format!("{escaped}%")
            } else {
                format!("%{escaped}")
            };
            params.push(SqlValue::Text(needle));
            " IS NOT NULL
                  AND LOWER(b.name) LIKE ? ESCAPE '\\'"
                .to_string()
        }
        SmartMailboxOperator::Regex => {
            params.push(SqlValue::Text(
                expect_string_value(&condition.value)?.to_string(),
            ));
            " IS NOT NULL
                  AND b.name REGEXP ?"
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
