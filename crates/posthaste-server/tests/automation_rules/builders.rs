use posthaste_domain::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, AutomationAction,
    AutomationRule, AutomationTrigger, MailboxId, MailboxRecord, MessageId, MessageRecord,
    SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
    SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue, ThreadId,
    RFC3339_EPOCH,
};

pub(super) fn account(id: &str, name: &str) -> AccountSettings {
    AccountSettings {
        id: AccountId::from(id),
        name: name.to_string(),
        full_name: None,
        email_patterns: Vec::new(),
        driver: AccountDriver::Mock,
        enabled: true,
        appearance: None,
        transport: AccountTransportSettings::default(),
        created_at: RFC3339_EPOCH.to_string(),
        updated_at: RFC3339_EPOCH.to_string(),
    }
}

pub(super) fn mailbox(id: &str, name: &str, role: Option<&str>) -> MailboxRecord {
    MailboxRecord {
        id: MailboxId::from(id),
        name: name.to_string(),
        role: role.map(str::to_string),
        unread_emails: 0,
        total_emails: 0,
    }
}

pub(super) fn message(
    id: &str,
    mailbox_ids: &[&str],
    from_name: &str,
    from_email: &str,
    keywords: &[&str],
) -> MessageRecord {
    MessageRecord {
        id: MessageId::from(id),
        source_thread_id: ThreadId::from(format!("thread-{id}")),
        subject: Some(format!("Subject {id}")),
        from_name: Some(from_name.to_string()),
        from_email: Some(from_email.to_string()),
        preview: Some(format!("Preview {id}")),
        received_at: "2026-03-31T10:00:00Z".to_string(),
        size: 42,
        mailbox_ids: mailbox_ids.iter().map(|id| MailboxId::from(*id)).collect(),
        keywords: keywords.iter().map(|keyword| keyword.to_string()).collect(),
        body_text: Some(format!("Body {id}")),
        rfc_message_id: Some(format!("<{id}@example.test>")),
        ..Default::default()
    }
}

fn condition(
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

pub(super) fn source_is(account_id: &str) -> SmartMailboxRuleNode {
    condition(
        SmartMailboxField::SourceId,
        SmartMailboxOperator::Equals,
        SmartMailboxValue::String(account_id.to_string()),
    )
}

pub(super) fn from_contains(value: &str) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Group(SmartMailboxGroup {
        operator: SmartMailboxGroupOperator::Any,
        negated: false,
        nodes: vec![
            condition(
                SmartMailboxField::FromName,
                SmartMailboxOperator::Contains,
                SmartMailboxValue::String(value.to_string()),
            ),
            condition(
                SmartMailboxField::FromEmail,
                SmartMailboxOperator::Contains,
                SmartMailboxValue::String(value.to_string()),
            ),
        ],
    })
}

pub(super) fn mailbox_role_is(role: &str) -> SmartMailboxRuleNode {
    condition(
        SmartMailboxField::MailboxRole,
        SmartMailboxOperator::Equals,
        SmartMailboxValue::String(role.to_string()),
    )
}

pub(super) fn rule(
    id: &str,
    nodes: Vec<SmartMailboxRuleNode>,
    actions: Vec<AutomationAction>,
) -> AutomationRule {
    AutomationRule {
        id: id.to_string(),
        name: id.to_string(),
        enabled: true,
        triggers: vec![AutomationTrigger::MessageArrived],
        condition: SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: SmartMailboxGroupOperator::All,
                negated: false,
                nodes,
            },
        },
        actions,
        backfill: true,
    }
}
