use super::field_compilers::{
    compile_bool_field, compile_date_field, compile_exists_membership,
    compile_exists_text_membership, compile_numeric_field, compile_recipient_json_field,
    compile_simple_field, compile_text_field,
};
use super::*;

/// Compiles a smart mailbox rule tree into a SQL WHERE clause with
/// parameterized bindings.
pub(crate) fn compile_smart_mailbox_rule(
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
///
/// Operator validity is decided **once**, up front, against the canonical
/// [`field_spec`] schema — the single source of truth shared with the web
/// registry. The per-field type-compilers below only encode *how* to compile
/// each operator (operator → SQL), not *which* are allowed, so the store
/// compiler and the schema can no longer disagree (a schema-rejected operator
/// never reaches a type-compiler; the `schema_and_compiler_agree_on_operators`
/// test pins the converse — every schema-allowed operator actually compiles).
fn compile_smart_mailbox_condition(
    condition: &SmartMailboxCondition,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    let spec = field_spec(condition.field);
    if !spec.operators.contains(&condition.operator) {
        return Err(StoreError::Failure(format!(
            "unsupported operator {:?} for field {:?}",
            condition.operator, condition.field
        )));
    }
    let fragment = match condition.field {
        SmartMailboxField::SourceId => compile_simple_field("m.account_id", condition, params)?,
        SmartMailboxField::SourceName => {
            compile_text_field("COALESCE(a.name, m.account_id)", condition, params)?
        }
        SmartMailboxField::MessageId => compile_simple_field("m.id", condition, params)?,
        SmartMailboxField::ThreadId => compile_simple_field("m.thread_id", condition, params)?,
        SmartMailboxField::ConversationId => {
            compile_simple_field("m.conversation_id", condition, params)?
        }
        SmartMailboxField::FromName => compile_text_field("m.from_name", condition, params)?,
        SmartMailboxField::FromEmail => compile_text_field("m.from_email", condition, params)?,
        SmartMailboxField::To => compile_recipient_json_field("m.to_json", condition, params)?,
        SmartMailboxField::Subject => compile_text_field("m.subject", condition, params)?,
        SmartMailboxField::Preview => compile_text_field("m.preview", condition, params)?,
        SmartMailboxField::ReceivedAt => compile_date_field("m.received_at", condition, params)?,
        SmartMailboxField::Size => compile_numeric_field("m.size", condition, params)?,
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

#[cfg(test)]
mod schema_agreement_tests {
    use super::*;
    use posthaste_domain_model::{DateValue, QueryValueType, ALL_QUERY_FIELDS};

    /// A correctly-shaped value for a `(field, operator)` pair, so a *valid*
    /// combination reaches the type-compiler's real logic rather than tripping a
    /// value-shape error.
    fn sample_value(
        value_type: QueryValueType,
        operator: SmartMailboxOperator,
    ) -> SmartMailboxValue {
        match value_type {
            QueryValueType::Bool => SmartMailboxValue::Bool(true),
            QueryValueType::Date => SmartMailboxValue::Date(DateValue::Absolute {
                value: "2026-07-06T00:00:00Z".to_string(),
            }),
            // `Size` stayed stringly (R5a's no-migration model): a byte count as text.
            QueryValueType::Number => SmartMailboxValue::String("100".to_string()),
            QueryValueType::Text => match operator {
                SmartMailboxOperator::In => SmartMailboxValue::Strings(vec!["x".to_string()]),
                _ => SmartMailboxValue::String("x".to_string()),
            },
        }
    }

    /// The schema and the store compiler agree on operator validity: for every
    /// field, every operator the schema ALLOWS compiles to SQL, and every
    /// operator the schema REJECTS is refused. This pins the two together so the
    /// single-source claim holds at runtime, not just by convention.
    #[test]
    fn schema_and_compiler_agree_on_operators() {
        let all_operators = [
            SmartMailboxOperator::Equals,
            SmartMailboxOperator::In,
            SmartMailboxOperator::Contains,
            SmartMailboxOperator::BeginsWith,
            SmartMailboxOperator::EndsWith,
            SmartMailboxOperator::Regex,
            SmartMailboxOperator::Lt,
            SmartMailboxOperator::Gt,
            SmartMailboxOperator::Le,
            SmartMailboxOperator::Ge,
        ];
        for &field in ALL_QUERY_FIELDS {
            let spec = field_spec(field);
            for operator in all_operators {
                let allowed = spec.operators.contains(&operator);
                let condition = SmartMailboxCondition {
                    field,
                    operator,
                    negated: false,
                    value: sample_value(spec.value_type, operator),
                };
                let mut params = Vec::new();
                let result = compile_smart_mailbox_condition(&condition, &mut params);
                assert_eq!(
                    result.is_ok(),
                    allowed,
                    "field {field:?} operator {operator:?}: schema allowed={allowed} but \
                     compiler ok={}",
                    result.is_ok()
                );
            }
        }
    }
}
