use mail_domain::{
    SmartMailbox, SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup,
    SmartMailboxGroupOperator, SmartMailboxId, SmartMailboxKind, SmartMailboxOperator,
    SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue, RFC3339_EPOCH,
};

/// Returns the built-in smart mailboxes: Inbox, Archive, Drafts, Sent, Junk,
/// Trash, and All Mail. Each filters by `mailbox_role`; All Mail uses an empty
/// rule (matches everything).
///
/// @spec spec/L1-accounts#smart-mailbox-defaults
pub fn default_smart_mailboxes() -> Vec<SmartMailbox> {
    vec![
        role_mailbox("default-inbox", "Inbox", 0, "inbox", "inbox"),
        role_mailbox("default-archive", "Archive", 1, "archive", "archive"),
        role_mailbox("default-drafts", "Drafts", 2, "drafts", "drafts"),
        role_mailbox("default-sent", "Sent", 3, "sent", "sent"),
        role_mailbox("default-junk", "Junk", 4, "junk", "junk"),
        role_mailbox("default-trash", "Trash", 5, "trash", "trash"),
        all_mail_mailbox(),
    ]
}

/// Constructs a default smart mailbox that filters messages by a single
/// `mailbox_role` condition.
fn role_mailbox(
    id: &str,
    name: &str,
    position: i64,
    default_key: &str,
    role: &str,
) -> SmartMailbox {
    let timestamp = RFC3339_EPOCH.to_string();
    SmartMailbox {
        id: SmartMailboxId::from(id),
        name: name.to_string(),
        position,
        kind: SmartMailboxKind::Default,
        default_key: Some(default_key.to_string()),
        parent_id: None,
        rule: SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: SmartMailboxGroupOperator::All,
                negated: false,
                nodes: vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                    field: SmartMailboxField::MailboxRole,
                    operator: SmartMailboxOperator::Equals,
                    negated: false,
                    value: SmartMailboxValue::String(role.to_string()),
                })],
            },
        },
        created_at: timestamp.clone(),
        updated_at: timestamp,
    }
}

/// Constructs the "All Mail" smart mailbox with an empty rule that matches
/// every message.
fn all_mail_mailbox() -> SmartMailbox {
    let timestamp = RFC3339_EPOCH.to_string();
    SmartMailbox {
        id: SmartMailboxId::from("default-all-mail"),
        name: "All Mail".to_string(),
        position: 6,
        kind: SmartMailboxKind::Default,
        default_key: Some("all-mail".to_string()),
        parent_id: None,
        rule: SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: SmartMailboxGroupOperator::All,
                negated: false,
                nodes: Vec::new(),
            },
        },
        created_at: timestamp.clone(),
        updated_at: timestamp,
    }
}
