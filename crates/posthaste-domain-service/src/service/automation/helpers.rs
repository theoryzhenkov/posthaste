use super::*;

fn condition_node(
    field: MailQueryField,
    operator: MailQueryOperator,
    value: MailQueryValue,
) -> MailQueryRuleNode {
    MailQueryRuleNode::Condition(MailQueryCondition {
        field,
        operator,
        negated: false,
        value,
    })
}

fn negated_condition_node(
    field: MailQueryField,
    operator: MailQueryOperator,
    value: MailQueryValue,
) -> MailQueryRuleNode {
    MailQueryRuleNode::Condition(MailQueryCondition {
        field,
        operator,
        negated: true,
        value,
    })
}

pub(super) fn automation_query_rule(
    account_id: &AccountId,
    rule: &AutomationRule,
    action: &AutomationAction,
    message_ids: &[MessageId],
) -> MailQueryRule {
    let mut nodes = vec![
        condition_node(
            MailQueryField::SourceId,
            MailQueryOperator::Equals,
            MailQueryValue::String(account_id.to_string()),
        ),
        MailQueryRuleNode::Group(rule.condition.root.clone()),
    ];

    if !message_ids.is_empty() {
        nodes.push(condition_node(
            MailQueryField::MessageId,
            MailQueryOperator::In,
            MailQueryValue::Strings(message_ids.iter().map(ToString::to_string).collect()),
        ));
    }

    if let Some(precondition) = automation_action_precondition(action) {
        nodes.push(precondition);
    }

    MailQueryRule {
        root: MailQueryGroup {
            operator: MailQueryGroupOperator::All,
            negated: false,
            nodes,
        },
    }
}

pub(super) fn automation_action_precondition(
    action: &AutomationAction,
) -> Option<MailQueryRuleNode> {
    match action {
        AutomationAction::ApplyTag { tag } => Some(negated_condition_node(
            MailQueryField::Keyword,
            MailQueryOperator::Equals,
            MailQueryValue::String(tag.clone()),
        )),
        AutomationAction::RemoveTag { tag } => Some(condition_node(
            MailQueryField::Keyword,
            MailQueryOperator::Equals,
            MailQueryValue::String(tag.clone()),
        )),
        AutomationAction::MarkRead => Some(condition_node(
            MailQueryField::IsRead,
            MailQueryOperator::Equals,
            MailQueryValue::Bool(false),
        )),
        AutomationAction::MarkUnread => Some(condition_node(
            MailQueryField::IsRead,
            MailQueryOperator::Equals,
            MailQueryValue::Bool(true),
        )),
        AutomationAction::Flag => Some(condition_node(
            MailQueryField::IsFlagged,
            MailQueryOperator::Equals,
            MailQueryValue::Bool(false),
        )),
        AutomationAction::Unflag => Some(condition_node(
            MailQueryField::IsFlagged,
            MailQueryOperator::Equals,
            MailQueryValue::Bool(true),
        )),
        AutomationAction::MoveToMailbox { mailbox_id } => Some(negated_condition_node(
            MailQueryField::MailboxId,
            MailQueryOperator::Equals,
            MailQueryValue::String(mailbox_id.to_string()),
        )),
    }
}

pub(super) fn automation_backfill_fingerprint(
    settings: &AppSettings,
) -> Result<Option<String>, ServiceError> {
    let rules = settings
        .automation_rules
        .iter()
        .filter(|rule| rule.enabled && rule.backfill)
        .cloned()
        .collect::<Vec<_>>();
    if rules.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(&rules).map(Some).map_err(|err| {
        StoreError::Failure(format!("failed to fingerprint automation rules: {err}")).into()
    })
}
