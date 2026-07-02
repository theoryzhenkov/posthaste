use super::*;

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

fn negated_condition_node(
    field: SmartMailboxField,
    operator: SmartMailboxOperator,
    value: SmartMailboxValue,
) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
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
) -> SmartMailboxRule {
    let mut nodes = vec![
        condition_node(
            SmartMailboxField::SourceId,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(account_id.to_string()),
        ),
        SmartMailboxRuleNode::Group(rule.condition.root.clone()),
    ];

    if !message_ids.is_empty() {
        nodes.push(condition_node(
            SmartMailboxField::MessageId,
            SmartMailboxOperator::In,
            SmartMailboxValue::Strings(message_ids.iter().map(ToString::to_string).collect()),
        ));
    }

    if let Some(precondition) = automation_action_precondition(action) {
        nodes.push(precondition);
    }

    SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes,
        },
    }
}

pub(super) fn automation_action_precondition(
    action: &AutomationAction,
) -> Option<SmartMailboxRuleNode> {
    match action {
        AutomationAction::ApplyTag { tag } => Some(negated_condition_node(
            SmartMailboxField::Keyword,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(tag.clone()),
        )),
        AutomationAction::RemoveTag { tag } => Some(condition_node(
            SmartMailboxField::Keyword,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(tag.clone()),
        )),
        AutomationAction::MarkRead => Some(condition_node(
            SmartMailboxField::IsRead,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(false),
        )),
        AutomationAction::MarkUnread => Some(condition_node(
            SmartMailboxField::IsRead,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(true),
        )),
        AutomationAction::Flag => Some(condition_node(
            SmartMailboxField::IsFlagged,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(false),
        )),
        AutomationAction::Unflag => Some(condition_node(
            SmartMailboxField::IsFlagged,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(true),
        )),
        AutomationAction::MoveToMailbox { mailbox_id } => Some(negated_condition_node(
            SmartMailboxField::MailboxId,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(mailbox_id.to_string()),
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
