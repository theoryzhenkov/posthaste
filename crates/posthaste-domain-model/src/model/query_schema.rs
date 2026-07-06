//! The single source of truth for the mail-query field schema: for every
//! [`SmartMailboxField`], its value *type* (the value-shape family the compiler
//! validates against) and the set of [`SmartMailboxOperator`]s it accepts.
//!
//! Before R5b this "field X is type Y, allows operators [...]" matrix was encoded
//! TWICE — once in the store SQL compiler (`rule_compiler.rs` dispatch + each
//! `field_compilers.rs` type-compiler's operator `match`) and once, by hand, in
//! the web `FIELD_REGISTRY`. They could drift, so the editor could offer an
//! operator the compiler rejected → a runtime `StoreError::Failure` deep in SQL.
//!
//! Now this table is canonical:
//! - the store compiler validates `condition.operator` against
//!   [`field_spec`]`(field).operators` **before** dispatching (no second
//!   per-type validity list that can disagree — the type-compilers only encode
//!   *how* to compile an operator, not *which* are allowed);
//! - the web `FIELD_REGISTRY` is *generated* from this table via the committed
//!   `query-schema.json` artifact (see [`query_schema_document`]) — the web can
//!   no longer hand-maintain a parallel matrix.
//!
//! @spec docs/L1-accounts#condition-fields-and-operators
//! @spec docs/eph/RFC-L2-query-schema.md#d4--one-canonical-field-schema

use serde::Serialize;

use super::smart_mailboxes::{
    DateValue, SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxOperator,
    SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue,
};

use SmartMailboxOperator::{Contains, Equals, Ge, Gt, In, Le, Lt};

/// The value-shape family a field's condition value carries. This selects how the
/// compiler validates and binds the value, and drives the web's coarse widget
/// choice (the web refines it with a presentation-only override map — e.g. `Text`
/// → an address picker for `fromEmail`).
///
/// This is deliberately *coarser* than the wire value enum: within [`Text`] the
/// operator distinguishes scalar (`equals`/`contains`) from list (`in`) shapes,
/// so a text field has one `value_type` regardless of operator.
///
/// [`Text`]: QueryValueType::Text
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum QueryValueType {
    /// String-shaped: `equals`/`contains` bind a scalar, `in` binds a list.
    Text,
    /// Boolean: `equals` against a 0/1 column.
    Bool,
    /// A typed date (absolute instant or rolling relative offset).
    Date,
    /// A numeric comparison. The wire value stays a byte-count *string* (R5a's
    /// additive, no-migration model was kept — `Size` was not retyped to a
    /// numeric wire value), but the compiler parses and compares it as an
    /// integer; `Number` marks that intent for the editor and the compiler.
    Number,
}

/// The canonical spec for a single query field: its value type and the exact set
/// of operators it accepts. Held as a `&'static` slice so the table is a `const`.
#[derive(Clone, Copy, Debug)]
pub struct MailQueryFieldSpec {
    pub value_type: QueryValueType,
    pub operators: &'static [SmartMailboxOperator],
}

// The operator sets, named by the value-shape they gate. These are the ONE place
// operator validity is declared; the store compiler reads them, the web registry
// is generated from them.

/// Equality / membership only (identifier-shaped columns: ids, roles, keywords).
const EQ_IN: &[SmartMailboxOperator] = &[Equals, In];
/// Equality / substring / membership (free-text columns).
const EQ_CONTAINS_IN: &[SmartMailboxOperator] = &[Equals, Contains, In];
/// Boolean equality.
const EQ_ONLY: &[SmartMailboxOperator] = &[Equals];
/// The four ordered comparisons, reused for dates and numbers (`< > <= >=`).
const ORDERED: &[SmartMailboxOperator] = &[Lt, Gt, Le, Ge];

/// The canonical field → `{ value_type, operators }` table. This match is the
/// single source of truth; the compiler and the generated web registry both
/// derive from it.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
pub const fn field_spec(field: SmartMailboxField) -> MailQueryFieldSpec {
    use SmartMailboxField as F;
    let (value_type, operators) = match field {
        // Identifier-shaped text columns: exact match or membership.
        F::SourceId
        | F::MessageId
        | F::ThreadId
        | F::ConversationId
        | F::MailboxId
        | F::MailboxRole
        | F::Keyword => (QueryValueType::Text, EQ_IN),
        // Free-text / address columns: also substring `contains`.
        F::SourceName
        | F::MailboxName
        | F::FromName
        | F::FromEmail
        | F::To
        | F::Subject
        | F::Preview => (QueryValueType::Text, EQ_CONTAINS_IN),
        // Boolean flags.
        F::IsRead | F::IsFlagged | F::HasAttachment => (QueryValueType::Bool, EQ_ONLY),
        // Date column: ordered comparisons.
        F::ReceivedAt => (QueryValueType::Date, ORDERED),
        // Numeric byte-size column: the same ordered comparisons, numeric semantics.
        F::Size => (QueryValueType::Number, ORDERED),
    };
    MailQueryFieldSpec {
        value_type,
        operators,
    }
}

/// Every query field, in a stable declaration order — the order the generated
/// `query-schema.json` artifact (and thus the web registry) enumerate. Keep this
/// exhaustive: [`all_fields_are_exhaustive`] asserts it covers the enum.
pub const ALL_QUERY_FIELDS: &[SmartMailboxField] = &[
    SmartMailboxField::SourceId,
    SmartMailboxField::SourceName,
    SmartMailboxField::MessageId,
    SmartMailboxField::ThreadId,
    SmartMailboxField::ConversationId,
    SmartMailboxField::MailboxId,
    SmartMailboxField::MailboxName,
    SmartMailboxField::MailboxRole,
    SmartMailboxField::IsRead,
    SmartMailboxField::IsFlagged,
    SmartMailboxField::HasAttachment,
    SmartMailboxField::Keyword,
    SmartMailboxField::FromName,
    SmartMailboxField::FromEmail,
    SmartMailboxField::To,
    SmartMailboxField::Subject,
    SmartMailboxField::Preview,
    SmartMailboxField::ReceivedAt,
    SmartMailboxField::Size,
];

/// One row of the serialized schema document (one field's spec), with the field
/// name and operators rendered in their camelCase wire form.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryFieldSchemaEntry {
    pub field: SmartMailboxField,
    pub value_type: QueryValueType,
    pub operators: Vec<SmartMailboxOperator>,
}

/// The serializable schema document emitted to `query-schema.json` and consumed
/// by the web codegen (`gen-query-schema.ts`). A stable, ordered mirror of
/// [`ALL_QUERY_FIELDS`] × [`field_spec`].
#[derive(Debug, Serialize)]
pub struct QuerySchemaDocument {
    pub fields: Vec<QueryFieldSchemaEntry>,
}

/// Build the canonical schema document from [`ALL_QUERY_FIELDS`] + [`field_spec`].
/// This is what the Rust contract test writes to the committed artifact and what
/// the web generator reads.
pub fn query_schema_document() -> QuerySchemaDocument {
    let fields = ALL_QUERY_FIELDS
        .iter()
        .map(|&field| {
            let spec = field_spec(field);
            QueryFieldSchemaEntry {
                field,
                value_type: spec.value_type,
                operators: spec.operators.to_vec(),
            }
        })
        .collect();
    QuerySchemaDocument { fields }
}

/// Pretty JSON for the committed `query-schema.json` artifact (trailing newline,
/// mirroring the openapi/asyncapi contract artifacts).
pub fn query_schema_json() -> String {
    let mut json = serde_json::to_string_pretty(&query_schema_document())
        .expect("query schema document should serialize");
    json.push('\n');
    json
}

// ---------------------------------------------------------------------------
// D5 — boundary validation
// ---------------------------------------------------------------------------

/// Why a [`SmartMailboxCondition`] failed [`validate_condition`]. Typed so the
/// boundary can map it to a stable, machine-readable reason (the API's
/// `query_invalid` body carries it alongside the field/operator).
///
/// @spec docs/eph/RFC-L2-query-schema.md#d5--boundary-validation-invalid--rejected-at-the-edge
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryValidationReason {
    /// The operator is not in the field's [`field_spec`] operator set.
    OperatorNotAllowed,
    /// The value's shape does not match the field's [`QueryValueType`].
    ValueTypeMismatch,
}

impl QueryValidationReason {
    /// A stable snake_case token for the reason (mirrors the serde wire form).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OperatorNotAllowed => "operator_not_allowed",
            Self::ValueTypeMismatch => "value_type_mismatch",
        }
    }
}

/// A condition rejected at the write boundary before it can be persisted (and
/// later fail deep in the store SQL compiler as a runtime `StoreError`). Carries
/// exactly which field + operator failed and why, so the API can surface a clear,
/// caller-actionable `query_invalid` error.
///
/// @spec docs/eph/RFC-L2-query-schema.md#d5--boundary-validation-invalid--rejected-at-the-edge
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryValidationError {
    pub field: SmartMailboxField,
    pub operator: SmartMailboxOperator,
    pub reason: QueryValidationReason,
}

impl std::fmt::Display for QueryValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.reason {
            QueryValidationReason::OperatorNotAllowed => write!(
                f,
                "operator {:?} is not allowed for field {:?}",
                self.operator, self.field
            ),
            QueryValidationReason::ValueTypeMismatch => write!(
                f,
                "value type does not match field {:?} (operator {:?})",
                self.field, self.operator
            ),
        }
    }
}

/// Whether a value's JSON *shape* matches a field's [`QueryValueType`]. This is
/// the same shape family the store compiler binds against, checked here up front
/// so a mismatch is a boundary error, not a deep `StoreError`.
///
/// `Date` accepts both the typed [`SmartMailboxValue::Date`] and a legacy bare
/// [`SmartMailboxValue::String`] (R5a kept legacy absolute date-strings readable);
/// `Number` is the stringly byte-count value R5a left untyped.
fn value_matches_type(value_type: QueryValueType, value: &SmartMailboxValue) -> bool {
    match value_type {
        QueryValueType::Text => matches!(
            value,
            SmartMailboxValue::String(_) | SmartMailboxValue::Strings(_)
        ),
        QueryValueType::Bool => matches!(value, SmartMailboxValue::Bool(_)),
        QueryValueType::Date => matches!(
            value,
            SmartMailboxValue::Date(DateValue::Absolute { .. } | DateValue::Relative { .. })
                | SmartMailboxValue::String(_)
        ),
        // `Size` stayed a byte-count string (R5a's no-migration model).
        QueryValueType::Number => matches!(value, SmartMailboxValue::String(_)),
    }
}

/// Validate a single leaf condition against the canonical [`field_spec`] schema:
/// (a) the operator must be in the field's operator set, and (b) the value's
/// shape must match the field's value type. Returns a typed
/// [`QueryValidationError`] carrying the field, operator, and reason.
///
/// @spec docs/eph/RFC-L2-query-schema.md#d5--boundary-validation-invalid--rejected-at-the-edge
pub fn validate_condition(condition: &SmartMailboxCondition) -> Result<(), QueryValidationError> {
    let spec = field_spec(condition.field);
    if !spec.operators.contains(&condition.operator) {
        return Err(QueryValidationError {
            field: condition.field,
            operator: condition.operator,
            reason: QueryValidationReason::OperatorNotAllowed,
        });
    }
    if !value_matches_type(spec.value_type, &condition.value) {
        return Err(QueryValidationError {
            field: condition.field,
            operator: condition.operator,
            reason: QueryValidationReason::ValueTypeMismatch,
        });
    }
    Ok(())
}

/// Recurse a rule group, validating every leaf condition and descending into
/// nested groups. The first invalid condition (depth-first) is returned.
fn validate_group(group: &SmartMailboxGroup) -> Result<(), QueryValidationError> {
    for node in &group.nodes {
        match node {
            SmartMailboxRuleNode::Group(child) => validate_group(child)?,
            SmartMailboxRuleNode::Condition(condition) => validate_condition(condition)?,
        }
    }
    Ok(())
}

/// Validate an entire query tree (a [`SmartMailboxRule`]) against the canonical
/// schema, recursing through nested groups. This is the authoritative boundary
/// check: a rule / smart mailbox that fails here must be rejected when SUBMITTED,
/// so an invalid query is never stored for the compiler to later reject.
///
/// @spec docs/eph/RFC-L2-query-schema.md#d5--boundary-validation-invalid--rejected-at-the-edge
pub fn validate_query(rule: &SmartMailboxRule) -> Result<(), QueryValidationError> {
    validate_group(&rule.root)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ALL_QUERY_FIELDS` must list every enum variant exactly once — this pins
    /// the generated artifact's field set to the domain enum. If a field is added
    /// to `SmartMailboxField`, this fails until it is given a spec + listed here.
    #[test]
    fn all_fields_are_exhaustive() {
        // Exhaustive match: adding a variant to `SmartMailboxField` breaks this
        // arm and forces the author to extend `ALL_QUERY_FIELDS` + `field_spec`.
        use SmartMailboxField as F;
        for &field in ALL_QUERY_FIELDS {
            match field {
                F::SourceId
                | F::SourceName
                | F::MessageId
                | F::ThreadId
                | F::ConversationId
                | F::MailboxId
                | F::MailboxName
                | F::MailboxRole
                | F::IsRead
                | F::IsFlagged
                | F::HasAttachment
                | F::Keyword
                | F::FromName
                | F::FromEmail
                | F::To
                | F::Subject
                | F::Preview
                | F::ReceivedAt
                | F::Size => {}
            }
        }
        // No duplicates, and every field has a non-empty operator set.
        for (i, &field) in ALL_QUERY_FIELDS.iter().enumerate() {
            assert!(
                !ALL_QUERY_FIELDS[..i].contains(&field),
                "duplicate field {field:?} in ALL_QUERY_FIELDS"
            );
            assert!(
                !field_spec(field).operators.is_empty(),
                "field {field:?} has no operators"
            );
        }
    }

    #[test]
    fn document_renders_camel_case_fields_and_operators() {
        let json = serde_json::to_value(query_schema_document()).unwrap();
        let first = &json["fields"][0];
        assert_eq!(first["field"], "sourceId");
        assert_eq!(first["valueType"], "text");
        assert_eq!(first["operators"], serde_json::json!(["equals", "in"]));
    }

    fn condition(
        field: SmartMailboxField,
        operator: SmartMailboxOperator,
        value: SmartMailboxValue,
    ) -> SmartMailboxCondition {
        SmartMailboxCondition {
            field,
            operator,
            negated: false,
            value,
        }
    }

    fn rule_of(nodes: Vec<SmartMailboxRuleNode>) -> SmartMailboxRule {
        SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: super::super::smart_mailboxes::SmartMailboxGroupOperator::All,
                negated: false,
                nodes,
            },
        }
    }

    #[test]
    fn condition_with_operator_not_in_field_set_is_rejected() {
        // `contains` is not allowed on a boolean field.
        let err = validate_condition(&condition(
            SmartMailboxField::IsRead,
            SmartMailboxOperator::Contains,
            SmartMailboxValue::Bool(true),
        ))
        .unwrap_err();
        assert_eq!(err.field, SmartMailboxField::IsRead);
        assert_eq!(err.operator, SmartMailboxOperator::Contains);
        assert_eq!(err.reason, QueryValidationReason::OperatorNotAllowed);
    }

    #[test]
    fn value_type_mismatch_is_rejected() {
        // A boolean field with a string value: valid operator, wrong value shape.
        let err = validate_condition(&condition(
            SmartMailboxField::IsRead,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String("nope".to_string()),
        ))
        .unwrap_err();
        assert_eq!(err.reason, QueryValidationReason::ValueTypeMismatch);
    }

    #[test]
    fn valid_conditions_pass_across_value_types() {
        // Text scalar, text list, bool, number-as-string, date typed + legacy string.
        for cond in [
            condition(
                SmartMailboxField::Subject,
                SmartMailboxOperator::Contains,
                SmartMailboxValue::String("hi".to_string()),
            ),
            condition(
                SmartMailboxField::Subject,
                SmartMailboxOperator::In,
                SmartMailboxValue::Strings(vec!["a".to_string()]),
            ),
            condition(
                SmartMailboxField::IsFlagged,
                SmartMailboxOperator::Equals,
                SmartMailboxValue::Bool(true),
            ),
            condition(
                SmartMailboxField::Size,
                SmartMailboxOperator::Gt,
                SmartMailboxValue::String("1024".to_string()),
            ),
            condition(
                SmartMailboxField::ReceivedAt,
                SmartMailboxOperator::Lt,
                SmartMailboxValue::Date(DateValue::Relative {
                    amount: 7,
                    unit: super::super::smart_mailboxes::DateUnit::Days,
                }),
            ),
            condition(
                SmartMailboxField::ReceivedAt,
                SmartMailboxOperator::Ge,
                SmartMailboxValue::String("2026-07-06T00:00:00Z".to_string()),
            ),
        ] {
            assert_eq!(validate_condition(&cond), Ok(()), "condition {cond:?}");
        }
    }

    #[test]
    fn validate_query_recurses_into_nested_groups() {
        use super::super::smart_mailboxes::SmartMailboxGroupOperator;
        // A valid outer condition and a nested group hiding an invalid one.
        let nested = SmartMailboxRuleNode::Group(SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::Any,
            negated: false,
            nodes: vec![SmartMailboxRuleNode::Condition(condition(
                // `before`/`lt` is not a valid operator for a boolean field.
                SmartMailboxField::HasAttachment,
                SmartMailboxOperator::Lt,
                SmartMailboxValue::Bool(true),
            ))],
        });
        let rule = rule_of(vec![
            SmartMailboxRuleNode::Condition(condition(
                SmartMailboxField::Subject,
                SmartMailboxOperator::Contains,
                SmartMailboxValue::String("ok".to_string()),
            )),
            nested,
        ]);
        let err = validate_query(&rule).unwrap_err();
        assert_eq!(err.field, SmartMailboxField::HasAttachment);
        assert_eq!(err.reason, QueryValidationReason::OperatorNotAllowed);

        // A fully valid tree passes.
        let ok = rule_of(vec![SmartMailboxRuleNode::Condition(condition(
            SmartMailboxField::Subject,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String("ok".to_string()),
        ))]);
        assert_eq!(validate_query(&ok), Ok(()));
    }
}
