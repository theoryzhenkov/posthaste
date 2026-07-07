//! The single source of truth for the mail-query field schema: for every
//! [`MailQueryField`], its value *type* (the value-shape family the compiler
//! validates against) and the set of [`MailQueryOperator`]s it accepts.
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

use super::mail_query::{
    DateValue, MailQueryCondition, MailQueryField, MailQueryGroup, MailQueryOperator,
    MailQueryRule, MailQueryRuleNode, MailQueryValue,
};

use MailQueryOperator::{BeginsWith, Contains, EndsWith, Equals, Ge, Gt, In, Le, Lt, Regex};

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
    pub operators: &'static [MailQueryOperator],
}

// The operator sets, named by the value-shape they gate. These are the ONE place
// operator validity is declared; the store compiler reads them, the web registry
// is generated from them.

/// Equality / membership only (identifier-shaped columns: ids, roles, keywords).
const EQ_IN: &[MailQueryOperator] = &[Equals, In];
/// Free-text columns: equality / substring / membership plus the additive text
/// match operators (R4) — prefix (`beginsWith`), suffix (`endsWith`), and regex.
/// These belong with `contains`: they are text-shaped predicates over the same
/// free-text columns (id columns keep the leaner [`EQ_IN`] set).
const EQ_CONTAINS_IN: &[MailQueryOperator] = &[Equals, Contains, In, BeginsWith, EndsWith, Regex];
/// `contains` only: the FTS-backed body field. The FTS5 index answers token /
/// phrase containment; equality, prefix/suffix, regex, and `in` have no
/// index-backed meaning over unstored (external-content) body text, so they
/// are not offered rather than silently falling back to a full-body scan.
const CONTAINS_ONLY: &[MailQueryOperator] = &[Contains];
/// Boolean equality.
const EQ_ONLY: &[MailQueryOperator] = &[Equals];
/// The four ordered comparisons, reused for dates and numbers (`< > <= >=`).
const ORDERED: &[MailQueryOperator] = &[Lt, Gt, Le, Ge];

/// The canonical field → `{ value_type, operators }` table. This match is the
/// single source of truth; the compiler and the generated web registry both
/// derive from it.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
pub const fn field_spec(field: MailQueryField) -> MailQueryFieldSpec {
    use MailQueryField as F;
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
        // Full-text body: token/phrase containment via the FTS5 index only.
        F::Body => (QueryValueType::Text, CONTAINS_ONLY),
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
pub const ALL_QUERY_FIELDS: &[MailQueryField] = &[
    MailQueryField::SourceId,
    MailQueryField::SourceName,
    MailQueryField::MessageId,
    MailQueryField::ThreadId,
    MailQueryField::ConversationId,
    MailQueryField::MailboxId,
    MailQueryField::MailboxName,
    MailQueryField::MailboxRole,
    MailQueryField::IsRead,
    MailQueryField::IsFlagged,
    MailQueryField::HasAttachment,
    MailQueryField::Keyword,
    MailQueryField::FromName,
    MailQueryField::FromEmail,
    MailQueryField::To,
    MailQueryField::Subject,
    MailQueryField::Preview,
    MailQueryField::Body,
    MailQueryField::ReceivedAt,
    MailQueryField::Size,
];

/// One row of the serialized schema document (one field's spec), with the field
/// name and operators rendered in their camelCase wire form.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryFieldSchemaEntry {
    pub field: MailQueryField,
    pub value_type: QueryValueType,
    pub operators: Vec<MailQueryOperator>,
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

/// Why a [`MailQueryCondition`] failed [`validate_condition`]. Typed so the
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
    /// A `regex` operator carried a pattern that does not compile. Rejected here
    /// so an un-compilable regex never reaches the store (where it would surface
    /// as a runtime error on every scanned row). The engine is the same `regex`
    /// crate the store's `regexp` SQL scalar uses, so validation agrees with
    /// evaluation.
    InvalidRegex,
}

impl QueryValidationReason {
    /// A stable snake_case token for the reason (mirrors the serde wire form).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OperatorNotAllowed => "operator_not_allowed",
            Self::ValueTypeMismatch => "value_type_mismatch",
            Self::InvalidRegex => "invalid_regex",
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
    pub field: MailQueryField,
    pub operator: MailQueryOperator,
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
            QueryValidationReason::InvalidRegex => write!(
                f,
                "value is not a valid regular expression for field {:?}",
                self.field
            ),
        }
    }
}

/// Whether a value's JSON *shape* matches a field's [`QueryValueType`]. This is
/// the same shape family the store compiler binds against, checked here up front
/// so a mismatch is a boundary error, not a deep `StoreError`.
///
/// `Date` accepts both the typed [`MailQueryValue::Date`] and a legacy bare
/// [`MailQueryValue::String`] (R5a kept legacy absolute date-strings readable);
/// `Number` is the stringly byte-count value R5a left untyped.
fn value_matches_type(value_type: QueryValueType, value: &MailQueryValue) -> bool {
    match value_type {
        QueryValueType::Text => matches!(
            value,
            MailQueryValue::String(_) | MailQueryValue::Strings(_)
        ),
        QueryValueType::Bool => matches!(value, MailQueryValue::Bool(_)),
        QueryValueType::Date => matches!(
            value,
            MailQueryValue::Date(DateValue::Absolute { .. } | DateValue::Relative { .. })
                | MailQueryValue::String(_)
        ),
        // `Size` stayed a byte-count string (R5a's no-migration model).
        QueryValueType::Number => matches!(value, MailQueryValue::String(_)),
    }
}

/// Validate a single leaf condition against the canonical [`field_spec`] schema:
/// (a) the operator must be in the field's operator set, and (b) the value's
/// shape must match the field's value type. Returns a typed
/// [`QueryValidationError`] carrying the field, operator, and reason.
///
/// @spec docs/eph/RFC-L2-query-schema.md#d5--boundary-validation-invalid--rejected-at-the-edge
pub fn validate_condition(condition: &MailQueryCondition) -> Result<(), QueryValidationError> {
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
    // A `regex` operator's value must be a single, compilable pattern. A list
    // (`Strings`) has no scalar pattern to compile, and a pattern that does not
    // compile must be rejected here rather than failing per-row in the store.
    if condition.operator == MailQueryOperator::Regex {
        match &condition.value {
            MailQueryValue::String(pattern) => {
                if regex::Regex::new(pattern).is_err() {
                    return Err(QueryValidationError {
                        field: condition.field,
                        operator: condition.operator,
                        reason: QueryValidationReason::InvalidRegex,
                    });
                }
            }
            _ => {
                return Err(QueryValidationError {
                    field: condition.field,
                    operator: condition.operator,
                    reason: QueryValidationReason::ValueTypeMismatch,
                });
            }
        }
    }
    Ok(())
}

/// Recurse a rule group, validating every leaf condition and descending into
/// nested groups. The first invalid condition (depth-first) is returned.
fn validate_group(group: &MailQueryGroup) -> Result<(), QueryValidationError> {
    for node in &group.nodes {
        match node {
            MailQueryRuleNode::Group(child) => validate_group(child)?,
            MailQueryRuleNode::Condition(condition) => validate_condition(condition)?,
        }
    }
    Ok(())
}

/// Validate an entire query tree (a [`MailQueryRule`]) against the canonical
/// schema, recursing through nested groups. This is the authoritative boundary
/// check: a rule / smart mailbox that fails here must be rejected when SUBMITTED,
/// so an invalid query is never stored for the compiler to later reject.
///
/// @spec docs/eph/RFC-L2-query-schema.md#d5--boundary-validation-invalid--rejected-at-the-edge
pub fn validate_query(rule: &MailQueryRule) -> Result<(), QueryValidationError> {
    validate_group(&rule.root)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ALL_QUERY_FIELDS` must list every enum variant exactly once — this pins
    /// the generated artifact's field set to the domain enum. If a field is added
    /// to `MailQueryField`, this fails until it is given a spec + listed here.
    #[test]
    fn all_fields_are_exhaustive() {
        // Exhaustive match: adding a variant to `MailQueryField` breaks this
        // arm and forces the author to extend `ALL_QUERY_FIELDS` + `field_spec`.
        use MailQueryField as F;
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
                | F::Body
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
        field: MailQueryField,
        operator: MailQueryOperator,
        value: MailQueryValue,
    ) -> MailQueryCondition {
        MailQueryCondition {
            field,
            operator,
            negated: false,
            value,
        }
    }

    fn rule_of(nodes: Vec<MailQueryRuleNode>) -> MailQueryRule {
        MailQueryRule {
            root: MailQueryGroup {
                operator: super::super::mail_query::MailQueryGroupOperator::All,
                negated: false,
                nodes,
            },
        }
    }

    #[test]
    fn condition_with_operator_not_in_field_set_is_rejected() {
        // `contains` is not allowed on a boolean field.
        let err = validate_condition(&condition(
            MailQueryField::IsRead,
            MailQueryOperator::Contains,
            MailQueryValue::Bool(true),
        ))
        .unwrap_err();
        assert_eq!(err.field, MailQueryField::IsRead);
        assert_eq!(err.operator, MailQueryOperator::Contains);
        assert_eq!(err.reason, QueryValidationReason::OperatorNotAllowed);
    }

    #[test]
    fn value_type_mismatch_is_rejected() {
        // A boolean field with a string value: valid operator, wrong value shape.
        let err = validate_condition(&condition(
            MailQueryField::IsRead,
            MailQueryOperator::Equals,
            MailQueryValue::String("nope".to_string()),
        ))
        .unwrap_err();
        assert_eq!(err.reason, QueryValidationReason::ValueTypeMismatch);
    }

    #[test]
    fn valid_conditions_pass_across_value_types() {
        // Text scalar, text list, bool, number-as-string, date typed + legacy string.
        for cond in [
            condition(
                MailQueryField::Subject,
                MailQueryOperator::Contains,
                MailQueryValue::String("hi".to_string()),
            ),
            condition(
                MailQueryField::Subject,
                MailQueryOperator::In,
                MailQueryValue::Strings(vec!["a".to_string()]),
            ),
            condition(
                MailQueryField::IsFlagged,
                MailQueryOperator::Equals,
                MailQueryValue::Bool(true),
            ),
            condition(
                MailQueryField::Size,
                MailQueryOperator::Gt,
                MailQueryValue::String("1024".to_string()),
            ),
            condition(
                MailQueryField::ReceivedAt,
                MailQueryOperator::Lt,
                MailQueryValue::Date(DateValue::Relative {
                    amount: 7,
                    unit: super::super::mail_query::DateUnit::Days,
                }),
            ),
            condition(
                MailQueryField::ReceivedAt,
                MailQueryOperator::Ge,
                MailQueryValue::String("2026-07-06T00:00:00Z".to_string()),
            ),
        ] {
            assert_eq!(validate_condition(&cond), Ok(()), "condition {cond:?}");
        }
    }

    #[test]
    fn valid_regex_pattern_passes_but_malformed_is_rejected() {
        // A well-formed anchored pattern on a text field validates.
        assert_eq!(
            validate_condition(&condition(
                MailQueryField::Subject,
                MailQueryOperator::Regex,
                MailQueryValue::String("^foo.*bar$".to_string()),
            )),
            Ok(())
        );
        // A malformed pattern (unclosed group) is a typed boundary error, NOT a
        // panic and NOT a deferred store failure.
        let err = validate_condition(&condition(
            MailQueryField::Subject,
            MailQueryOperator::Regex,
            MailQueryValue::String("foo(".to_string()),
        ))
        .unwrap_err();
        assert_eq!(err.reason, QueryValidationReason::InvalidRegex);
        assert_eq!(err.operator, MailQueryOperator::Regex);
        // A regex operator needs a scalar pattern — a list is a type mismatch.
        let err = validate_condition(&condition(
            MailQueryField::Subject,
            MailQueryOperator::Regex,
            MailQueryValue::Strings(vec!["a".to_string()]),
        ))
        .unwrap_err();
        assert_eq!(err.reason, QueryValidationReason::ValueTypeMismatch);
    }

    #[test]
    fn text_match_operators_pass_on_free_text_fields() {
        for operator in [MailQueryOperator::BeginsWith, MailQueryOperator::EndsWith] {
            assert_eq!(
                validate_condition(&condition(
                    MailQueryField::FromEmail,
                    operator,
                    MailQueryValue::String("50%".to_string()),
                )),
                Ok(())
            );
        }
        // …but not on an identifier field (which keeps the leaner equals/in set).
        let err = validate_condition(&condition(
            MailQueryField::MessageId,
            MailQueryOperator::Regex,
            MailQueryValue::String("x".to_string()),
        ))
        .unwrap_err();
        assert_eq!(err.reason, QueryValidationReason::OperatorNotAllowed);
    }

    #[test]
    fn validate_query_recurses_into_nested_groups() {
        use super::super::mail_query::MailQueryGroupOperator;
        // A valid outer condition and a nested group hiding an invalid one.
        let nested = MailQueryRuleNode::Group(MailQueryGroup {
            operator: MailQueryGroupOperator::Any,
            negated: false,
            nodes: vec![MailQueryRuleNode::Condition(condition(
                // `before`/`lt` is not a valid operator for a boolean field.
                MailQueryField::HasAttachment,
                MailQueryOperator::Lt,
                MailQueryValue::Bool(true),
            ))],
        });
        let rule = rule_of(vec![
            MailQueryRuleNode::Condition(condition(
                MailQueryField::Subject,
                MailQueryOperator::Contains,
                MailQueryValue::String("ok".to_string()),
            )),
            nested,
        ]);
        let err = validate_query(&rule).unwrap_err();
        assert_eq!(err.field, MailQueryField::HasAttachment);
        assert_eq!(err.reason, QueryValidationReason::OperatorNotAllowed);

        // A fully valid tree passes.
        let ok = rule_of(vec![MailQueryRuleNode::Condition(condition(
            MailQueryField::Subject,
            MailQueryOperator::Equals,
            MailQueryValue::String("ok".to_string()),
        ))]);
        assert_eq!(validate_query(&ok), Ok(()));
    }
}
