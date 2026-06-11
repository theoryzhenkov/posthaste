use super::*;

pub(crate) fn convert_automation_rule(rule: &AutomationRuleToml) -> Result<AutomationRule, String> {
    Ok(AutomationRule {
        id: rule.id.clone(),
        name: rule.name.clone(),
        enabled: rule.enabled,
        triggers: rule
            .triggers
            .iter()
            .map(convert_automation_trigger)
            .collect(),
        condition: SmartMailboxRule {
            root: convert_rule_group(&rule.condition)?,
        },
        actions: rule.actions.iter().map(convert_automation_action).collect(),
        backfill: rule.backfill,
    })
}

pub(crate) fn convert_automation_trigger(trigger: &AutomationTriggerToml) -> AutomationTrigger {
    match trigger {
        AutomationTriggerToml::MessageArrived => AutomationTrigger::MessageArrived,
        AutomationTriggerToml::MessageChanged => AutomationTrigger::MessageChanged,
        AutomationTriggerToml::Manual => AutomationTrigger::Manual,
    }
}

pub(crate) fn convert_automation_action(action: &AutomationActionToml) -> AutomationAction {
    match action {
        AutomationActionToml::ApplyTag { tag } => AutomationAction::ApplyTag { tag: tag.clone() },
        AutomationActionToml::RemoveTag { tag } => AutomationAction::RemoveTag { tag: tag.clone() },
        AutomationActionToml::MarkRead => AutomationAction::MarkRead,
        AutomationActionToml::MarkUnread => AutomationAction::MarkUnread,
        AutomationActionToml::Flag => AutomationAction::Flag,
        AutomationActionToml::Unflag => AutomationAction::Unflag,
        AutomationActionToml::MoveToMailbox { mailbox_id } => AutomationAction::MoveToMailbox {
            mailbox_id: MailboxId::from(mailbox_id.as_str()),
        },
    }
}

pub(crate) fn convert_automation_rule_to_toml(rule: &AutomationRule) -> AutomationRuleToml {
    AutomationRuleToml {
        id: rule.id.clone(),
        name: rule.name.clone(),
        enabled: rule.enabled,
        triggers: rule
            .triggers
            .iter()
            .map(convert_automation_trigger_to_toml)
            .collect(),
        backfill: rule.backfill,
        condition: convert_group_to_toml(&rule.condition.root),
        actions: rule
            .actions
            .iter()
            .map(convert_automation_action_to_toml)
            .collect(),
    }
}

pub(crate) fn convert_automation_trigger_to_toml(
    trigger: &AutomationTrigger,
) -> AutomationTriggerToml {
    match trigger {
        AutomationTrigger::MessageArrived => AutomationTriggerToml::MessageArrived,
        AutomationTrigger::MessageChanged => AutomationTriggerToml::MessageChanged,
        AutomationTrigger::Manual => AutomationTriggerToml::Manual,
    }
}

pub(crate) fn convert_automation_action_to_toml(action: &AutomationAction) -> AutomationActionToml {
    match action {
        AutomationAction::ApplyTag { tag } => AutomationActionToml::ApplyTag { tag: tag.clone() },
        AutomationAction::RemoveTag { tag } => AutomationActionToml::RemoveTag { tag: tag.clone() },
        AutomationAction::MarkRead => AutomationActionToml::MarkRead,
        AutomationAction::MarkUnread => AutomationActionToml::MarkUnread,
        AutomationAction::Flag => AutomationActionToml::Flag,
        AutomationAction::Unflag => AutomationActionToml::Unflag,
        AutomationAction::MoveToMailbox { mailbox_id } => AutomationActionToml::MoveToMailbox {
            mailbox_id: mailbox_id.to_string(),
        },
    }
}
