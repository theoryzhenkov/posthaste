use std::collections::BTreeSet;

use crate::ConfigSnapshot;
use posthaste_domain_model::{
    AccountAppearance, AccountDriver, AccountId, AccountSettings, AutomationAction, AutomationRule,
    ImapTransportSettings, SmartMailboxId, SmtpTransportSettings, ValidationError,
};

pub fn validate_snapshot(snapshot: &ConfigSnapshot) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    let source_ids = collect_source_ids(&snapshot.sources, &mut errors);
    collect_smart_mailbox_ids(&snapshot.smart_mailboxes, &mut errors);

    if let Some(default_account_id) = &snapshot.app_settings.default_account_id {
        if let Err(error) = validate_default_account_exists(
            default_account_id,
            source_ids.contains(default_account_id),
        ) {
            errors.push(error);
        }
    }

    for source in &snapshot.sources {
        errors.extend(account_settings_errors(source));
    }
    errors.extend(automation_rule_errors(
        &snapshot.app_settings.automation_rules,
    ));
    errors.extend(automation_draft_errors(
        &snapshot.app_settings.automation_rules,
        &snapshot.app_settings.automation_drafts,
    ));

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn validate_default_account_exists(
    account_id: &AccountId,
    exists: bool,
) -> Result<(), ValidationError> {
    if exists {
        Ok(())
    } else {
        Err(ValidationError::DanglingDefaultAccount(
            account_id.as_str().to_string(),
        ))
    }
}

pub fn validate_account_settings(account: &AccountSettings) -> Result<(), ValidationError> {
    account_settings_errors(account)
        .into_iter()
        .next()
        .map_or(Ok(()), Err)
}

pub fn validate_automation_rules(rules: &[AutomationRule]) -> Result<(), ValidationError> {
    automation_rule_errors(rules)
        .into_iter()
        .next()
        .map_or(Ok(()), Err)
}

pub fn validate_automation_drafts(
    active_rules: &[AutomationRule],
    draft_rules: &[AutomationRule],
) -> Result<(), ValidationError> {
    automation_draft_errors(active_rules, draft_rules)
        .into_iter()
        .next()
        .map_or(Ok(()), Err)
}

fn collect_source_ids(
    sources: &[AccountSettings],
    errors: &mut Vec<ValidationError>,
) -> BTreeSet<AccountId> {
    let mut ids = BTreeSet::new();
    for source in sources {
        if !ids.insert(source.id.clone()) {
            errors.push(ValidationError::DuplicateSourceId(
                source.id.as_str().to_string(),
            ));
        }
    }
    ids
}

fn collect_smart_mailbox_ids(
    smart_mailboxes: &[posthaste_domain_model::SmartMailbox],
    errors: &mut Vec<ValidationError>,
) -> BTreeSet<SmartMailboxId> {
    let mut ids = BTreeSet::new();
    for mailbox in smart_mailboxes {
        if !ids.insert(mailbox.id.clone()) {
            errors.push(ValidationError::DuplicateSmartMailboxId(
                mailbox.id.as_str().to_string(),
            ));
        }
    }
    ids
}

fn account_settings_errors(account: &AccountSettings) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    if account.id.as_str().trim().is_empty() {
        errors.push(ValidationError::InvalidAccount(
            "account id is required".to_string(),
        ));
    }
    if account.name.trim().is_empty() {
        errors.push(ValidationError::InvalidAccount(
            "account name is required".to_string(),
        ));
    }
    if account
        .email_patterns
        .iter()
        .any(|pattern| pattern.trim().is_empty())
    {
        errors.push(ValidationError::InvalidAccount(
            "email patterns must not be blank".to_string(),
        ));
    }
    if matches!(account.driver, AccountDriver::Jmap) {
        if account
            .transport
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            errors.push(ValidationError::BaseUrlRequired(
                "JMAP base URL is required".to_string(),
            ));
        }
        if account.transport.secret_ref.is_none() {
            errors.push(ValidationError::SecretRequired(
                "JMAP secret must be configured before saving the account".to_string(),
            ));
        }
    }
    if matches!(account.driver, AccountDriver::ImapSmtp) {
        if account
            .transport
            .username
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            errors.push(ValidationError::UsernameRequired(
                "IMAP/SMTP username is required".to_string(),
            ));
        }
        if account.transport.secret_ref.is_none() {
            errors.push(ValidationError::SecretRequired(
                "IMAP/SMTP secret must be configured before saving the account".to_string(),
            ));
        }
        endpoint_errors("IMAP", account.transport.imap.as_ref(), &mut errors);
        endpoint_errors("SMTP", account.transport.smtp.as_ref(), &mut errors);
        if !account
            .email_patterns
            .iter()
            .any(|pattern| is_concrete_email_pattern(pattern))
        {
            errors.push(ValidationError::SenderRequired(
                "IMAP/SMTP accounts require a concrete sender email pattern".to_string(),
            ));
        }
    }
    if let Some(
        AccountAppearance::Initials { initials, .. } | AccountAppearance::Image { initials, .. },
    ) = &account.appearance
    {
        if initials.trim().is_empty() {
            errors.push(ValidationError::InvalidAccount(
                "account appearance initials are required".to_string(),
            ));
        }
    }
    errors
}

trait EndpointLike {
    fn host(&self) -> &str;
    fn port(&self) -> u16;
}
impl EndpointLike for ImapTransportSettings {
    fn host(&self) -> &str {
        &self.host
    }
    fn port(&self) -> u16 {
        self.port
    }
}
impl EndpointLike for SmtpTransportSettings {
    fn host(&self) -> &str {
        &self.host
    }
    fn port(&self) -> u16 {
        self.port
    }
}

fn endpoint_errors<T: EndpointLike>(
    label: &str,
    endpoint: Option<&T>,
    errors: &mut Vec<ValidationError>,
) {
    let Some(endpoint) = endpoint else {
        errors.push(ValidationError::InvalidAccount(format!(
            "{label} endpoint is required"
        )));
        return;
    };
    if endpoint.host().trim().is_empty() {
        errors.push(ValidationError::InvalidAccount(format!(
            "{label} host is required"
        )));
    }
    if endpoint.port() == 0 {
        errors.push(ValidationError::InvalidAccount(format!(
            "{label} port must be greater than zero"
        )));
    }
}

fn is_concrete_email_pattern(pattern: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern.contains('*') {
        return false;
    }
    pattern
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
}

fn automation_rule_errors(rules: &[AutomationRule]) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let mut ids = BTreeSet::new();
    for rule in rules {
        if rule.id.trim().is_empty() {
            errors.push(ValidationError::InvalidAccount(
                "automation rule id is required".to_string(),
            ));
        } else if !ids.insert(rule.id.trim().to_string()) {
            errors.push(ValidationError::InvalidAccount(
                "automation rule ids must be unique".to_string(),
            ));
        }
        if rule.name.trim().is_empty() {
            errors.push(ValidationError::InvalidAccount(
                "automation rule name is required".to_string(),
            ));
        }
        if rule.triggers.is_empty() {
            errors.push(ValidationError::InvalidAccount(
                "automation rule must include at least one trigger".to_string(),
            ));
        }
        if rule.actions.is_empty() {
            errors.push(ValidationError::InvalidAccount(
                "automation rule must include at least one action".to_string(),
            ));
        }
        for action in &rule.actions {
            match action {
                AutomationAction::ApplyTag { tag } | AutomationAction::RemoveTag { tag }
                    if tag.trim().is_empty() || tag.starts_with('$') =>
                {
                    errors.push(ValidationError::InvalidAccount(
                        "automation tag must be a non-system keyword".to_string(),
                    ));
                }
                AutomationAction::MoveToMailbox { mailbox_id }
                    if mailbox_id.as_str().trim().is_empty() =>
                {
                    errors.push(ValidationError::InvalidAccount(
                        "automation target mailbox id is required".to_string(),
                    ));
                }
                _ => {}
            }
        }
    }
    errors
}

fn automation_draft_errors(
    active_rules: &[AutomationRule],
    draft_rules: &[AutomationRule],
) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let mut ids = BTreeSet::new();
    for rule in active_rules {
        ids.insert(rule.id.trim().to_string());
    }
    for rule in draft_rules {
        if rule.id.trim().is_empty() {
            errors.push(ValidationError::InvalidAccount(
                "automation draft id is required".to_string(),
            ));
        } else if !ids.insert(rule.id.trim().to_string()) {
            errors.push(ValidationError::InvalidAccount(
                "automation rule and draft ids must be unique".to_string(),
            ));
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_domain_model::{
        AccountTransportSettings, AppSettings, MailQueryGroup, MailQueryGroupOperator,
        MailQueryRule, SecretKind, SmartMailbox, SmartMailboxKind, TransportSecurity,
    };

    fn valid_account(id: &str) -> AccountSettings {
        AccountSettings {
            id: AccountId::from(id),
            name: "Account".to_string(),
            full_name: None,
            signature: None,
            email_patterns: Vec::new(),
            driver: AccountDriver::Mock,
            enabled: true,
            appearance: None,
            transport: AccountTransportSettings::default(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    fn valid_snapshot() -> ConfigSnapshot {
        ConfigSnapshot {
            app_settings: AppSettings::default(),
            sources: vec![valid_account("primary")],
            smart_mailboxes: Vec::new(),
        }
    }

    fn empty_rule() -> MailQueryRule {
        MailQueryRule {
            root: MailQueryGroup {
                operator: MailQueryGroupOperator::All,
                negated: false,
                nodes: Vec::new(),
            },
        }
    }

    fn smart_mailbox(id: &str) -> SmartMailbox {
        SmartMailbox {
            id: SmartMailboxId::from(id),
            name: "Mailbox".to_string(),
            kind: SmartMailboxKind::User,
            default_key: None,
            role: None,
            parent_id: None,
            rule: empty_rule(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    fn validation_errors(snapshot: &ConfigSnapshot) -> Vec<ValidationError> {
        validate_snapshot(snapshot).expect_err("snapshot should be rejected")
    }

    // spec: docs/authority-server/L2#domain-config-validation-source
    #[test]
    fn valid_snapshot_passes_validation() {
        assert_eq!(validate_snapshot(&valid_snapshot()), Ok(()));
    }

    #[test]
    fn snapshot_rejects_duplicate_source_ids() {
        let mut snapshot = valid_snapshot();
        snapshot.sources.push(valid_account("primary"));

        let errors = validation_errors(&snapshot);

        assert!(errors.contains(&ValidationError::DuplicateSourceId("primary".to_string())));
    }

    #[test]
    fn snapshot_rejects_duplicate_smart_mailbox_ids() {
        let mut snapshot = valid_snapshot();
        snapshot.smart_mailboxes = vec![smart_mailbox("inbox"), smart_mailbox("inbox")];

        let errors = validation_errors(&snapshot);

        assert!(errors.contains(&ValidationError::DuplicateSmartMailboxId(
            "inbox".to_string()
        )));
    }

    #[test]
    fn snapshot_rejects_dangling_default_account() {
        let mut snapshot = valid_snapshot();
        snapshot.app_settings.default_account_id = Some(AccountId::from("missing"));

        let errors = validation_errors(&snapshot);

        assert!(errors.contains(&ValidationError::DanglingDefaultAccount(
            "missing".to_string()
        )));
    }

    #[test]
    fn snapshot_rejects_invalid_account_settings() {
        let mut snapshot = valid_snapshot();
        snapshot.sources[0].name = " ".to_string();

        let errors = validation_errors(&snapshot);

        assert!(errors.contains(&ValidationError::InvalidAccount(
            "account name is required".to_string()
        )));
    }

    #[test]
    fn snapshot_rejects_incomplete_imap_smtp_settings() {
        let mut snapshot = valid_snapshot();
        snapshot.sources[0].driver = AccountDriver::ImapSmtp;
        snapshot.sources[0].email_patterns = vec!["primary@example.com".to_string()];

        let errors = validation_errors(&snapshot);

        assert!(errors.contains(&ValidationError::UsernameRequired(
            "IMAP/SMTP username is required".to_string()
        )));
        assert!(errors.contains(&ValidationError::SecretRequired(
            "IMAP/SMTP secret must be configured before saving the account".to_string()
        )));
        assert!(errors.contains(&ValidationError::InvalidAccount(
            "IMAP endpoint is required".to_string()
        )));
        assert!(errors.contains(&ValidationError::InvalidAccount(
            "SMTP endpoint is required".to_string()
        )));
    }

    #[test]
    fn account_settings_accept_complete_imap_smtp_settings() {
        let mut account = valid_account("imap");
        account.driver = AccountDriver::ImapSmtp;
        account.email_patterns = vec!["primary@example.com".to_string()];
        account.transport.username = Some("primary@example.com".to_string());
        account.transport.secret_ref = Some(posthaste_domain_model::SecretRef {
            kind: SecretKind::Env,
            key: "POSTHASTE_PASSWORD".to_string(),
        });
        account.transport.imap = Some(ImapTransportSettings {
            host: "imap.example.com".to_string(),
            port: 993,
            security: TransportSecurity::Tls,
        });
        account.transport.smtp = Some(SmtpTransportSettings {
            host: "smtp.example.com".to_string(),
            port: 465,
            security: TransportSecurity::Tls,
        });

        assert_eq!(validate_account_settings(&account), Ok(()));
    }

    #[test]
    fn snapshot_rejects_invalid_automation_rules_and_drafts() {
        let mut snapshot = valid_snapshot();
        snapshot.app_settings.automation_rules = vec![AutomationRule {
            id: "rule".to_string(),
            name: " ".to_string(),
            enabled: true,
            triggers: Vec::new(),
            condition: empty_rule(),
            actions: vec![AutomationAction::ApplyTag {
                tag: "$system".to_string(),
            }],
            backfill: false,
        }];
        snapshot.app_settings.automation_drafts = vec![AutomationRule {
            id: "rule".to_string(),
            name: "Draft".to_string(),
            enabled: false,
            triggers: Vec::new(),
            condition: empty_rule(),
            actions: Vec::new(),
            backfill: false,
        }];

        let errors = validation_errors(&snapshot);

        assert!(errors.contains(&ValidationError::InvalidAccount(
            "automation rule name is required".to_string()
        )));
        assert!(errors.contains(&ValidationError::InvalidAccount(
            "automation rule must include at least one trigger".to_string()
        )));
        assert!(errors.contains(&ValidationError::InvalidAccount(
            "automation tag must be a non-system keyword".to_string()
        )));
        assert!(errors.contains(&ValidationError::InvalidAccount(
            "automation rule and draft ids must be unique".to_string()
        )));
    }
}
