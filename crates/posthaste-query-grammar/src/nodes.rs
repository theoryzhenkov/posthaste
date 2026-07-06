use super::date::{date_node, relative_date_node};
use super::tokenizer::Token;
use super::*;

pub(super) fn parse_token(token: &Token) -> Result<Vec<SmartMailboxRuleNode>, String> {
    match token.prefix.as_deref() {
        Some(prefix) => parse_prefixed(prefix, &token.value, token.negated),
        None => Ok(vec![free_text_node(&token.value, token.negated)]),
    }
}

fn parse_prefixed(
    prefix: &str,
    value: &str,
    negated: bool,
) -> Result<Vec<SmartMailboxRuleNode>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("empty value for {prefix}:"));
    }
    let normalized_prefix = prefix.to_ascii_lowercase();
    match prefix {
        _ if matches!(normalized_prefix.as_str(), "f" | "from" | "sender") => {
            Ok(vec![from_node(value, negated)])
        }
        _ if matches!(normalized_prefix.as_str(), "subject" | "s") => Ok(vec![condition_node(
            SmartMailboxField::Subject,
            SmartMailboxOperator::Contains,
            SmartMailboxValue::String(value.to_string()),
            negated,
        )]),
        _ if matches!(normalized_prefix.as_str(), "body" | "preview") => Ok(vec![condition_node(
            SmartMailboxField::Preview,
            SmartMailboxOperator::Contains,
            SmartMailboxValue::String(value.to_string()),
            negated,
        )]),
        _ if normalized_prefix == "is" => is_node(value, negated),
        _ if normalized_prefix == "has" => has_node(value, negated),
        _ if matches!(normalized_prefix.as_str(), "tag" | "keyword") => Ok(vec![condition_node(
            SmartMailboxField::Keyword,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(value.to_string()),
            negated,
        )]),
        _ if matches!(normalized_prefix.as_str(), "in" | "mailbox") => {
            Ok(vec![mailbox_node(value, negated)])
        }
        _ if matches!(normalized_prefix.as_str(), "source" | "account") => {
            Ok(vec![source_node(value, negated)])
        }
        _ if normalized_prefix == "id" => Ok(vec![condition_node(
            SmartMailboxField::MessageId,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::String(value.to_string()),
            negated,
        )]),
        _ if matches!(normalized_prefix.as_str(), "thread" | "threadid") => {
            Ok(vec![condition_node(
                SmartMailboxField::ThreadId,
                SmartMailboxOperator::Equals,
                SmartMailboxValue::String(value.to_string()),
                negated,
            )])
        }
        _ if matches!(
            normalized_prefix.as_str(),
            "conversation" | "conversationid" | "conv"
        ) =>
        {
            Ok(vec![condition_node(
                SmartMailboxField::ConversationId,
                SmartMailboxOperator::Equals,
                SmartMailboxValue::String(value.to_string()),
                negated,
            )])
        }
        _ if normalized_prefix == "before" => Ok(vec![condition_node(
            SmartMailboxField::ReceivedAt,
            SmartMailboxOperator::Lt,
            SmartMailboxValue::String(value.to_string()),
            negated,
        )]),
        _ if normalized_prefix == "after" => Ok(vec![condition_node(
            SmartMailboxField::ReceivedAt,
            SmartMailboxOperator::Gt,
            SmartMailboxValue::String(value.to_string()),
            negated,
        )]),
        _ if normalized_prefix == "date" => date_node(value, negated),
        _ if normalized_prefix == "newer" => {
            relative_date_node(value, SmartMailboxOperator::Gt, negated)
        }
        _ if normalized_prefix == "older" => {
            relative_date_node(value, SmartMailboxOperator::Lt, negated)
        }
        _ => Err(format!("unknown search prefix: {prefix}")),
    }
}

// -- helpers ----------------------------------------------------------------

pub(super) fn condition_node(
    field: SmartMailboxField,
    operator: SmartMailboxOperator,
    value: SmartMailboxValue,
    negated: bool,
) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
        field,
        operator,
        negated,
        value,
    })
}

/// `from:value` -> ANY(FromEmail contains, FromName contains)
fn from_node(value: &str, negated: bool) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Group(SmartMailboxGroup {
        operator: SmartMailboxGroupOperator::Any,
        negated,
        nodes: vec![
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::FromEmail,
                operator: SmartMailboxOperator::Contains,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::FromName,
                operator: SmartMailboxOperator::Contains,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
        ],
    })
}

/// `in:value` -> ANY(mailbox role exact, mailbox id exact, mailbox name contains)
fn mailbox_node(value: &str, negated: bool) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Group(SmartMailboxGroup {
        operator: SmartMailboxGroupOperator::Any,
        negated,
        nodes: vec![
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::MailboxRole,
                operator: SmartMailboxOperator::Equals,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::MailboxId,
                operator: SmartMailboxOperator::Equals,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::MailboxName,
                operator: SmartMailboxOperator::Contains,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
        ],
    })
}

/// `source:value` -> ANY(source id exact, source display name contains)
fn source_node(value: &str, negated: bool) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Group(SmartMailboxGroup {
        operator: SmartMailboxGroupOperator::Any,
        negated,
        nodes: vec![
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::SourceId,
                operator: SmartMailboxOperator::Equals,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::SourceName,
                operator: SmartMailboxOperator::Contains,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
        ],
    })
}

/// `is:unread` / `is:flagged`
fn is_node(value: &str, negated: bool) -> Result<Vec<SmartMailboxRuleNode>, String> {
    let value = value.to_ascii_lowercase();
    match value.as_str() {
        "unread" => Ok(vec![condition_node(
            SmartMailboxField::IsRead,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(false),
            negated,
        )]),
        "read" | "seen" => Ok(vec![condition_node(
            SmartMailboxField::IsRead,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(true),
            negated,
        )]),
        "flagged" => Ok(vec![condition_node(
            SmartMailboxField::IsFlagged,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(true),
            negated,
        )]),
        "unflagged" => Ok(vec![condition_node(
            SmartMailboxField::IsFlagged,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(false),
            negated,
        )]),
        "attachment" | "attachments" => Ok(vec![condition_node(
            SmartMailboxField::HasAttachment,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(true),
            negated,
        )]),
        _ => Err(format!("unknown is: value: {value}")),
    }
}

/// `has:attachment`
fn has_node(value: &str, negated: bool) -> Result<Vec<SmartMailboxRuleNode>, String> {
    let value = value.to_ascii_lowercase();
    match value.as_str() {
        "attachment" | "attachments" => Ok(vec![condition_node(
            SmartMailboxField::HasAttachment,
            SmartMailboxOperator::Equals,
            SmartMailboxValue::Bool(true),
            negated,
        )]),
        _ => Err(format!("unknown has: value: {value}")),
    }
}

/// Free text: search across FromName, FromEmail, Subject, Preview.
fn free_text_node(value: &str, negated: bool) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Group(SmartMailboxGroup {
        operator: SmartMailboxGroupOperator::Any,
        negated,
        nodes: vec![
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::FromName,
                operator: SmartMailboxOperator::Contains,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::FromEmail,
                operator: SmartMailboxOperator::Contains,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::Subject,
                operator: SmartMailboxOperator::Contains,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                field: SmartMailboxField::Preview,
                operator: SmartMailboxOperator::Contains,
                negated: false,
                value: SmartMailboxValue::String(value.to_string()),
            }),
        ],
    })
}
