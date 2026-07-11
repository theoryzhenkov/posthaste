use super::field_compilers::{
    compile_body_fts_field, compile_bool_field, compile_date_field, compile_exists_membership,
    compile_exists_text_membership, compile_numeric_field, compile_recipient_json_field,
    compile_simple_field, compile_text_field,
};
use super::*;

/// Compiles a smart mailbox rule tree into a SQL WHERE clause with
/// parameterized bindings.
pub(crate) fn compile_mail_query_rule(
    rule: &MailQueryRule,
    params: &mut Vec<SqlValue>,
) -> Result<String, StoreError> {
    compile_mail_query_group(&rule.root, params)
}

/// Recursively compiles a rule group into SQL, joining nodes with AND/OR and
/// optionally wrapping in NOT.
fn compile_mail_query_group(
    group: &MailQueryGroup,
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
        MailQueryGroupOperator::All => " AND ",
        MailQueryGroupOperator::Any => " OR ",
    };
    let mut parts = Vec::with_capacity(group.nodes.len());
    for node in &group.nodes {
        let fragment = match node {
            MailQueryRuleNode::Group(group) => compile_mail_query_group(group, params)?,
            MailQueryRuleNode::Condition(condition) => {
                compile_mail_query_condition(condition, params)?
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
fn compile_mail_query_condition(
    condition: &MailQueryCondition,
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
        MailQueryField::SourceId => compile_simple_field("m.account_id", condition, params)?,
        MailQueryField::SourceName => {
            compile_text_field("COALESCE(a.name, m.account_id)", condition, params)?
        }
        MailQueryField::MessageId => compile_simple_field("m.id", condition, params)?,
        MailQueryField::ThreadId => compile_simple_field("m.thread_id", condition, params)?,
        MailQueryField::ConversationId => {
            compile_simple_field("m.conversation_id", condition, params)?
        }
        MailQueryField::FromName => compile_text_field("m.from_name", condition, params)?,
        MailQueryField::FromEmail => compile_text_field("m.from_email", condition, params)?,
        MailQueryField::To => compile_recipient_json_field("m.to_json", condition, params)?,
        MailQueryField::Subject => compile_text_field("m.subject", condition, params)?,
        MailQueryField::Preview => compile_text_field("m.preview", condition, params)?,
        MailQueryField::Body => compile_body_fts_field(condition, params)?,
        MailQueryField::ReceivedAt => compile_date_field("m.received_at", condition, params)?,
        MailQueryField::Size => compile_numeric_field("m.size", condition, params)?,
        MailQueryField::IsRead => compile_bool_field("m.is_read", condition)?,
        MailQueryField::IsFlagged => compile_bool_field("m.is_flagged", condition)?,
        MailQueryField::HasAttachment => compile_bool_field("m.has_attachment", condition)?,
        MailQueryField::MailboxId => compile_exists_membership(
            "EXISTS (
                SELECT 1
                FROM message_mailbox_effective mm
                WHERE mm.account_id = m.account_id
                  AND mm.message_id = m.id
                  AND mm.mailbox_id",
            condition,
            params,
        )?,
        MailQueryField::MailboxName => compile_exists_text_membership(
            "EXISTS (
                SELECT 1
                FROM message_mailbox_effective mm
                JOIN mailbox b
                  ON b.account_id = mm.account_id
                 AND b.id = mm.mailbox_id
                WHERE mm.account_id = m.account_id
                  AND mm.message_id = m.id
                  AND b.name",
            condition,
            params,
        )?,
        MailQueryField::Keyword => compile_exists_membership(
            "EXISTS (
                SELECT 1
                FROM message_keyword_effective mk
                WHERE mk.account_id = m.account_id
                  AND mk.message_id = m.id
                  AND mk.keyword",
            condition,
            params,
        )?,
        MailQueryField::MailboxRole => compile_exists_membership(
            "EXISTS (
                SELECT 1
                FROM message_mailbox_effective mm
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
    fn sample_value(value_type: QueryValueType, operator: MailQueryOperator) -> MailQueryValue {
        match value_type {
            QueryValueType::Bool => MailQueryValue::Bool(true),
            QueryValueType::Date => MailQueryValue::Date(DateValue::Absolute {
                value: "2026-07-06T00:00:00Z".to_string(),
            }),
            // `Size` stayed stringly (R5a's no-migration model): a byte count as text.
            QueryValueType::Number => MailQueryValue::String("100".to_string()),
            QueryValueType::Text => match operator {
                MailQueryOperator::In => MailQueryValue::Strings(vec!["x".to_string()]),
                _ => MailQueryValue::String("x".to_string()),
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
            MailQueryOperator::Equals,
            MailQueryOperator::In,
            MailQueryOperator::Contains,
            MailQueryOperator::BeginsWith,
            MailQueryOperator::EndsWith,
            MailQueryOperator::Regex,
            MailQueryOperator::Lt,
            MailQueryOperator::Gt,
            MailQueryOperator::Le,
            MailQueryOperator::Ge,
        ];
        for &field in ALL_QUERY_FIELDS {
            let spec = field_spec(field);
            for operator in all_operators {
                let allowed = spec.operators.contains(&operator);
                let condition = MailQueryCondition {
                    field,
                    operator,
                    negated: false,
                    value: sample_value(spec.value_type, operator),
                };
                let mut params = Vec::new();
                let result = compile_mail_query_condition(&condition, &mut params);
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
