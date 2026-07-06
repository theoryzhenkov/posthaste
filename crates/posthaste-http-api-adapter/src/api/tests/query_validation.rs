//! D5 boundary-validation tests: an invalid smart-mailbox / rule query is
//! rejected at the write boundary with a `query_invalid` 4xx carrying the
//! offending field/operator/reason — NOT persisted for the store SQL compiler to
//! later fail on as a runtime error.

use super::*;
use posthaste_domain_model::{
    SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
    SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue,
};

fn rule(nodes: Vec<SmartMailboxRuleNode>) -> SmartMailboxRule {
    SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes,
        },
    }
}

fn condition_node(
    field: SmartMailboxField,
    operator: SmartMailboxOperator,
    value: SmartMailboxValue,
) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
        field,
        operator,
        negated: false,
        value,
    })
}

#[test]
fn operator_not_in_field_set_is_rejected_with_query_invalid() {
    // `contains` is not a valid operator for a boolean field — rejected at the
    // boundary, not deep in the store compiler.
    let error = ApiError::validate_query(&rule(vec![condition_node(
        SmartMailboxField::IsRead,
        SmartMailboxOperator::Contains,
        SmartMailboxValue::Bool(true),
    )]))
    .expect_err("operator not in the field set should be rejected");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.body.code, ApiErrorCode::QueryInvalid);
    assert_eq!(error.body.details["field"], "isRead");
    assert_eq!(error.body.details["operator"], "contains");
    assert_eq!(error.body.details["reason"], "operator_not_allowed");
}

#[test]
fn value_type_mismatch_is_rejected() {
    // A boolean field with a string value: valid operator, wrong value shape.
    let error = ApiError::validate_query(&rule(vec![condition_node(
        SmartMailboxField::IsRead,
        SmartMailboxOperator::Equals,
        SmartMailboxValue::String("yes".to_string()),
    )]))
    .expect_err("value-type mismatch should be rejected");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.body.code, ApiErrorCode::QueryInvalid);
    assert_eq!(error.body.details["reason"], "value_type_mismatch");
}

#[test]
fn valid_query_passes_the_boundary() {
    let ok = rule(vec![condition_node(
        SmartMailboxField::Subject,
        SmartMailboxOperator::Contains,
        SmartMailboxValue::String("invoice".to_string()),
    )]);
    assert!(ApiError::validate_query(&ok).is_ok());
}

#[test]
fn invalid_condition_nested_in_a_group_is_rejected() {
    let nested = SmartMailboxRuleNode::Group(SmartMailboxGroup {
        operator: SmartMailboxGroupOperator::Any,
        negated: false,
        nodes: vec![condition_node(
            SmartMailboxField::Size,
            // `contains` is not valid for a numeric field.
            SmartMailboxOperator::Contains,
            SmartMailboxValue::String("100".to_string()),
        )],
    });
    let error = ApiError::validate_query(&rule(vec![
        condition_node(
            SmartMailboxField::Subject,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String("ok".to_string()),
        ),
        nested,
    ]))
    .expect_err("an invalid condition nested in a group should be rejected");

    assert_eq!(error.body.code, ApiErrorCode::QueryInvalid);
    assert_eq!(error.body.details["field"], "size");
    assert_eq!(error.body.details["reason"], "operator_not_allowed");
}
