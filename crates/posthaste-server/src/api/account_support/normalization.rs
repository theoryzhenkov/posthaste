use super::*;

/// Trim whitespace from an optional string, converting empty/blank to `None`.
pub(crate) fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Sparse-merge patch fields into an existing account. Omitted fields
/// (including transport sub-fields) are preserved.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[cfg(test)]
pub(crate) fn apply_account_patch(account: &mut AccountSettings, request: &PatchAccountRequest) {
    if let Some(name) = &request.name {
        account.name = name.trim().to_string();
    }
    if let Some(full_name) = &request.full_name {
        account.full_name = normalize_optional(Some(full_name.clone()));
    }
    if let Some(email_patterns) = &request.email_patterns {
        account.email_patterns = normalize_email_patterns(email_patterns);
    }
    if let Some(driver) = &request.driver {
        account.driver = driver.clone();
    }
    if let Some(enabled) = request.enabled {
        account.enabled = enabled;
    }
    if let Some(appearance) = &request.appearance {
        account.appearance = Some(normalize_account_appearance(appearance.clone()));
    }
    if let Some(transport) = &request.transport {
        if let Some(provider) = &transport.provider {
            account.transport.provider = provider.clone();
        }
        if let Some(auth) = &transport.auth {
            account.transport.auth = auth.clone();
        }
        if transport.base_url.is_some() {
            account.transport.base_url = normalize_optional(transport.base_url.clone());
        }
        if transport.username.is_some() {
            account.transport.username = normalize_optional(transport.username.clone());
        }
        if transport.imap.is_some() {
            account.transport.imap = transport.imap.clone();
        }
        if transport.smtp.is_some() {
            account.transport.smtp = transport.smtp.clone();
        }
    }
}

pub(crate) fn normalize_automation_rules(rules: &[AutomationRule]) -> Vec<AutomationRule> {
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

pub(crate) fn validate_automation_drafts(
    active_rules: &[AutomationRule],
    draft_rules: &[AutomationRule],
) -> Result<(), ApiError> {
    let mut ids = std::collections::BTreeSet::new();
    for rule in active_rules {
        ids.insert(rule.id.trim().to_string());
    }
    for rule in draft_rules {
        if rule.id.trim().is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidAccount,
                "automation draft id is required",
            ));
        }
        if !ids.insert(rule.id.trim().to_string()) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidAccount,
                "automation rule and draft ids must be unique",
            ));
        }
    }
    Ok(())
}

/// Normalize user-owned email addresses/patterns by trimming whitespace and
/// dropping empty entries. Patterns such as `*@example.com` are preserved.
#[cfg(test)]
pub(crate) fn normalize_email_patterns(patterns: &[String]) -> Vec<String> {
    patterns
        .iter()
        .filter_map(|pattern| {
            let trimmed = pattern.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect()
}
