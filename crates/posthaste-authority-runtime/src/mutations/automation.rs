use super::*;

impl AccountMutationService {
    pub fn preview_automation_rule(
        &self,
        request: AutomationRulePreviewMutation,
    ) -> Result<AutomationRulePreviewResult, RuntimeError> {
        let (_, total) = self.service.count_messages_by_rule(&request.condition)?;
        let page = self.service.query_message_page_by_rule(
            &request.condition,
            request.limit,
            None,
            MessageSortField::Date,
            SortDirection::Desc,
        )?;
        Ok(AutomationRulePreviewResult {
            total,
            items: page.items,
        })
    }
}

pub(super) fn normalize_automation_rules(rules: &[AutomationRule]) -> Vec<AutomationRule> {
    rules
        .iter()
        .map(|rule| AutomationRule {
            id: rule.id.trim().to_string(),
            name: rule.name.trim().to_string(),
            enabled: rule.enabled,
            triggers: rule.triggers.clone(),
            condition: rule.condition.clone(),
            actions: rule
                .actions
                .iter()
                .map(normalize_automation_action)
                .collect(),
            backfill: rule.backfill,
        })
        .collect()
}

fn normalize_automation_action(action: &AutomationAction) -> AutomationAction {
    match action {
        AutomationAction::ApplyTag { tag } => AutomationAction::ApplyTag {
            tag: tag.trim().to_string(),
        },
        AutomationAction::RemoveTag { tag } => AutomationAction::RemoveTag {
            tag: tag.trim().to_string(),
        },
        AutomationAction::MarkRead => AutomationAction::MarkRead,
        AutomationAction::MarkUnread => AutomationAction::MarkUnread,
        AutomationAction::Flag => AutomationAction::Flag,
        AutomationAction::Unflag => AutomationAction::Unflag,
        AutomationAction::MoveToMailbox { mailbox_id } => AutomationAction::MoveToMailbox {
            mailbox_id: MailboxId::from(mailbox_id.as_str().trim()),
        },
    }
}

pub(super) fn validate_automation_rules(rules: &[AutomationRule]) -> Result<(), RuntimeError> {
    let mut ids = std::collections::BTreeSet::new();
    for rule in rules {
        if rule.id.trim().is_empty() {
            return Err(RuntimeError::invalid_account(
                "automation rule id is required",
            ));
        }
        if !ids.insert(rule.id.trim().to_string()) {
            return Err(RuntimeError::invalid_account(
                "automation rule ids must be unique",
            ));
        }
        if rule.name.trim().is_empty() {
            return Err(RuntimeError::invalid_account(
                "automation rule name is required",
            ));
        }
        if rule.triggers.is_empty() {
            return Err(RuntimeError::invalid_account(
                "automation rule must include at least one trigger",
            ));
        }
        if rule.actions.is_empty() {
            return Err(RuntimeError::invalid_account(
                "automation rule must include at least one action",
            ));
        }
        for action in &rule.actions {
            match action {
                AutomationAction::ApplyTag { tag } | AutomationAction::RemoveTag { tag }
                    if tag.trim().is_empty() || tag.starts_with('$') =>
                {
                    return Err(RuntimeError::invalid_account(
                        "automation tag must be a non-system keyword",
                    ));
                }
                AutomationAction::MoveToMailbox { mailbox_id }
                    if mailbox_id.as_str().trim().is_empty() =>
                {
                    return Err(RuntimeError::invalid_account(
                        "automation target mailbox id is required",
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

pub(super) fn validate_automation_drafts(
    active_rules: &[AutomationRule],
    draft_rules: &[AutomationRule],
) -> Result<(), RuntimeError> {
    let mut ids = std::collections::BTreeSet::new();
    for rule in active_rules {
        ids.insert(rule.id.trim().to_string());
    }
    for rule in draft_rules {
        if rule.id.trim().is_empty() {
            return Err(RuntimeError::invalid_account(
                "automation draft id is required",
            ));
        }
        if !ids.insert(rule.id.trim().to_string()) {
            return Err(RuntimeError::invalid_account(
                "automation rule and draft ids must be unique",
            ));
        }
    }
    Ok(())
}
