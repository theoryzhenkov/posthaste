mod tokenize;

use posthaste_domain::{
    AccountId, MailService, MailboxId, SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup,
    SmartMailboxGroupOperator, SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode,
    SmartMailboxValue,
};
use posthaste_runtime_contract::RuntimeError;

use tokenize::tokenize;

pub(crate) fn compile(
    service: &MailService,
    query: &str,
) -> Result<SmartMailboxRule, RuntimeError> {
    let mut rules = Vec::new();
    let mut passthrough = Vec::new();
    for token in tokenize(query) {
        if token
            .prefix
            .as_deref()
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("in"))
        {
            let mut rule = resolve_in(service, &token.value)?;
            if token.negated {
                rule.root.negated = !rule.root.negated;
            }
            rules.push(rule);
        } else {
            passthrough.push(token.raw);
        }
    }
    let remaining = passthrough.join(" ");
    if !remaining.trim().is_empty() {
        rules.push(
            posthaste_domain::search::parse_query(&remaining)
                .map_err(RuntimeError::invalid_descriptor)?,
        );
    }
    Ok(combine(rules).unwrap_or_else(empty_rule))
}

fn resolve_in(service: &MailService, value: &str) -> Result<SmartMailboxRule, RuntimeError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(RuntimeError::invalid_descriptor(
            "in: selector cannot be empty",
        ));
    }
    if let Some((account, mailbox)) = value.split_once('/') {
        let account = account.trim();
        if account.is_empty() {
            return Err(RuntimeError::invalid_descriptor(
                "in: source selector cannot be empty",
            ));
        }
        let account_id = AccountId::from(account);
        let mailbox = mailbox.trim();
        let mailbox_id = (!mailbox.is_empty()).then(|| MailboxId::from(mailbox));
        return Ok(source_scope_rule(&account_id, mailbox_id.as_ref()));
    }
    service
        .find_smart_mailbox(value)?
        .map(|mailbox| mailbox.rule)
        .ok_or_else(|| RuntimeError::not_found(format!("smart mailbox not found: {value}")))
}

pub(crate) fn source_scope_rule(
    account_id: &AccountId,
    mailbox_id: Option<&MailboxId>,
) -> SmartMailboxRule {
    let mut nodes = vec![condition(
        SmartMailboxField::SourceId,
        SmartMailboxOperator::Equals,
        account_id.as_str(),
    )];
    if let Some(mailbox_id) = mailbox_id {
        nodes.push(condition(
            SmartMailboxField::MailboxId,
            SmartMailboxOperator::Equals,
            mailbox_id.as_str(),
        ));
    }
    all_rule(nodes)
}

pub(crate) fn combine(mut rules: Vec<SmartMailboxRule>) -> Option<SmartMailboxRule> {
    match rules.len() {
        0 => None,
        1 => Some(rules.remove(0)),
        _ => Some(all_rule(
            rules
                .into_iter()
                .map(|rule| SmartMailboxRuleNode::Group(rule.root))
                .collect(),
        )),
    }
}

fn condition(
    field: SmartMailboxField,
    operator: SmartMailboxOperator,
    value: impl Into<String>,
) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
        field,
        operator,
        negated: false,
        value: SmartMailboxValue::String(value.into()),
    })
}

fn all_rule(nodes: Vec<SmartMailboxRuleNode>) -> SmartMailboxRule {
    SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes,
        },
    }
}

fn empty_rule() -> SmartMailboxRule {
    all_rule(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_preserves_single_rule_root_semantics() {
        let rule = any_subject_or_sender_rule();
        let combined = combine(vec![rule]).expect("rule should combine");
        assert_eq!(combined.root.operator, SmartMailboxGroupOperator::Any);
        assert!(!combined.root.negated);
    }

    #[test]
    fn combine_groups_rules_without_flattening_saved_queries() {
        let combined = combine(vec![
            any_subject_or_sender_rule(),
            source_scope_rule(&AccountId::from("acct-a"), None),
        ])
        .expect("rules should combine");
        assert_eq!(combined.root.operator, SmartMailboxGroupOperator::All);
        assert_eq!(combined.root.nodes.len(), 2);
        let SmartMailboxRuleNode::Group(saved_query_group) = &combined.root.nodes[0] else {
            panic!("expected saved query to remain grouped");
        };
        assert_eq!(saved_query_group.operator, SmartMailboxGroupOperator::Any);
    }

    fn any_subject_or_sender_rule() -> SmartMailboxRule {
        SmartMailboxRule {
            root: SmartMailboxGroup {
                operator: SmartMailboxGroupOperator::Any,
                negated: false,
                nodes: vec![
                    condition(
                        SmartMailboxField::Subject,
                        SmartMailboxOperator::Contains,
                        "invoice",
                    ),
                    condition(
                        SmartMailboxField::FromEmail,
                        SmartMailboxOperator::Contains,
                        "billing@example.test",
                    ),
                ],
            },
        }
    }
}
