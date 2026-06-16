use posthaste_domain::{
    AccountId, MailService, MailboxId, SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup,
    SmartMailboxGroupOperator, SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode,
    SmartMailboxValue,
};
use posthaste_runtime_contract::{RuntimeAdapterError, RuntimeError, RuntimeErrorCode};

use crate::account_mutations::service_error_to_runtime_error;

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
            && !token.negated
        {
            rules.push(resolve_in(service, &token.value)?);
        } else {
            passthrough.push(token.raw);
        }
    }
    let remaining = passthrough.join(" ");
    if !remaining.trim().is_empty() {
        rules.push(
            posthaste_domain::search::parse_query(&remaining)
                .map_err(|message| runtime_error(RuntimeErrorCode::InvalidDescriptor, message))?,
        );
    }
    Ok(combine(rules).unwrap_or_else(empty_rule))
}

fn resolve_in(service: &MailService, value: &str) -> Result<SmartMailboxRule, RuntimeError> {
    let value = value.trim();
    if let Some((account, mailbox)) = value.split_once('/') {
        return Ok(source_scope_rule(
            &AccountId::from(account.trim()),
            Some(&MailboxId::from(mailbox.trim())),
        ));
    }
    service
        .find_smart_mailbox(value)
        .map_err(service_error_to_runtime_error)?
        .map(|mailbox| mailbox.rule)
        .ok_or_else(|| {
            runtime_error(
                RuntimeErrorCode::NotFound,
                format!("smart mailbox not found: {value}"),
            )
        })
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

pub(crate) fn combine(rules: Vec<SmartMailboxRule>) -> Option<SmartMailboxRule> {
    let mut nodes: Vec<_> = rules.into_iter().flat_map(|rule| rule.root.nodes).collect();
    match nodes.len() {
        0 => None,
        1 => Some(all_rule(vec![nodes.remove(0)])),
        _ => Some(all_rule(nodes)),
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

fn runtime_error(code: RuntimeErrorCode, message: impl Into<String>) -> RuntimeError {
    RuntimeError(RuntimeAdapterError {
        code,
        message: message.into(),
        retryable: false,
        correlation_id: None,
        details: serde_json::Value::Null,
    })
}

struct QueryToken {
    raw: String,
    negated: bool,
    prefix: Option<String>,
    value: String,
}

fn tokenize(input: &str) -> Vec<QueryToken> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let token_start = i;
        let negated = chars[i] == '-' && i + 1 < chars.len() && !chars[i + 1].is_whitespace();
        if negated {
            i += 1;
        }
        let prefix_start = i;
        let mut colon = None;
        while i < chars.len() && !chars[i].is_whitespace() {
            if chars[i] == ':' {
                colon = Some(i);
                break;
            }
            i += 1;
        }
        let (prefix, value) = if let Some(colon) = colon {
            let prefix = chars[prefix_start..colon].iter().collect::<String>();
            i = colon + 1;
            (Some(prefix), scan_value(&chars, &mut i))
        } else {
            i = prefix_start;
            (None, scan_value(&chars, &mut i))
        };
        let raw = chars[token_start..i].iter().collect::<String>();
        tokens.push(QueryToken {
            raw,
            negated,
            prefix,
            value,
        });
    }
    tokens
}

fn scan_value(chars: &[char], pos: &mut usize) -> String {
    if *pos < chars.len() && chars[*pos] == '"' {
        *pos += 1;
        let start = *pos;
        while *pos < chars.len() && chars[*pos] != '"' {
            *pos += 1;
        }
        let value = chars[start..*pos].iter().collect();
        if *pos < chars.len() {
            *pos += 1;
        }
        return value;
    }
    let start = *pos;
    while *pos < chars.len() && !chars[*pos].is_whitespace() {
        *pos += 1;
    }
    chars[start..*pos].iter().collect()
}
