use posthaste_contract_core::RuntimeError;
use posthaste_domain_model::{
    AccountId, MailQueryCondition, MailQueryField, MailQueryGroup, MailQueryGroupOperator,
    MailQueryOperator, MailQueryRule, MailQueryRuleNode, MailQueryValue, MailboxId,
};
use posthaste_domain_service::MailService;
use posthaste_query_grammar::parse_query_with_scopes;

/// Prefixes that name a mailbox *scope* rather than a searchable field —
/// resolved service-side by [`resolve_in`] instead of becoming a rule node.
/// Kept here (not in the domain parser) because scope resolution needs the
/// [`MailService`], which the service-free parser must not depend on.
const SCOPE_PREFIXES: &[&str] = &["in"];

pub(crate) fn compile(service: &MailService, query: &str) -> Result<MailQueryRule, RuntimeError> {
    let (remainder, scopes) =
        parse_query_with_scopes(query, SCOPE_PREFIXES).map_err(RuntimeError::invalid_descriptor)?;

    let mut rules = Vec::new();
    for scope in scopes {
        let mut rule = resolve_in(service, &scope.value)?;
        if scope.negated {
            rule.root.negated = !rule.root.negated;
        }
        rules.push(rule);
    }
    if let Some(remainder) = remainder {
        rules.push(remainder);
    }
    Ok(combine(rules).unwrap_or_else(empty_rule))
}

fn resolve_in(service: &MailService, value: &str) -> Result<MailQueryRule, RuntimeError> {
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
) -> MailQueryRule {
    let mut nodes = vec![condition(
        MailQueryField::SourceId,
        MailQueryOperator::Equals,
        account_id.as_str(),
    )];
    if let Some(mailbox_id) = mailbox_id {
        nodes.push(condition(
            MailQueryField::MailboxId,
            MailQueryOperator::Equals,
            mailbox_id.as_str(),
        ));
    }
    all_rule(nodes)
}

pub(crate) fn combine(mut rules: Vec<MailQueryRule>) -> Option<MailQueryRule> {
    match rules.len() {
        0 => None,
        1 => Some(rules.remove(0)),
        _ => Some(all_rule(
            rules
                .into_iter()
                .map(|rule| MailQueryRuleNode::Group(rule.root))
                .collect(),
        )),
    }
}

fn condition(
    field: MailQueryField,
    operator: MailQueryOperator,
    value: impl Into<String>,
) -> MailQueryRuleNode {
    MailQueryRuleNode::Condition(MailQueryCondition {
        field,
        operator,
        negated: false,
        value: MailQueryValue::String(value.into()),
    })
}

fn all_rule(nodes: Vec<MailQueryRuleNode>) -> MailQueryRule {
    MailQueryRule {
        root: MailQueryGroup {
            operator: MailQueryGroupOperator::All,
            negated: false,
            nodes,
        },
    }
}

fn empty_rule() -> MailQueryRule {
    all_rule(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_preserves_single_rule_root_semantics() {
        let rule = any_subject_or_sender_rule();
        let combined = combine(vec![rule]).expect("rule should combine");
        assert_eq!(combined.root.operator, MailQueryGroupOperator::Any);
        assert!(!combined.root.negated);
    }

    #[test]
    fn combine_groups_rules_without_flattening_saved_queries() {
        let combined = combine(vec![
            any_subject_or_sender_rule(),
            source_scope_rule(&AccountId::from("acct-a"), None),
        ])
        .expect("rules should combine");
        assert_eq!(combined.root.operator, MailQueryGroupOperator::All);
        assert_eq!(combined.root.nodes.len(), 2);
        let MailQueryRuleNode::Group(saved_query_group) = &combined.root.nodes[0] else {
            panic!("expected saved query to remain grouped");
        };
        assert_eq!(saved_query_group.operator, MailQueryGroupOperator::Any);
    }

    fn any_subject_or_sender_rule() -> MailQueryRule {
        MailQueryRule {
            root: MailQueryGroup {
                operator: MailQueryGroupOperator::Any,
                negated: false,
                nodes: vec![
                    condition(
                        MailQueryField::Subject,
                        MailQueryOperator::Contains,
                        "invoice",
                    ),
                    condition(
                        MailQueryField::FromEmail,
                        MailQueryOperator::Contains,
                        "billing@example.test",
                    ),
                ],
            },
        }
    }
}
