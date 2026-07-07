use super::date::{date_node, relative_date_node};
use super::tokenizer::Token;
use super::*;

pub(super) fn parse_token(token: &Token) -> Result<Vec<MailQueryRuleNode>, String> {
    match token.prefix.as_deref() {
        Some(prefix) => parse_prefixed(prefix, &token.value, token.negated),
        None => Ok(vec![free_text_node(&token.value, token.negated)]),
    }
}

fn parse_prefixed(
    prefix: &str,
    value: &str,
    negated: bool,
) -> Result<Vec<MailQueryRuleNode>, String> {
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
            MailQueryField::Subject,
            MailQueryOperator::Contains,
            MailQueryValue::String(value.to_string()),
            negated,
        )]),
        _ if matches!(normalized_prefix.as_str(), "body" | "preview") => Ok(vec![condition_node(
            MailQueryField::Preview,
            MailQueryOperator::Contains,
            MailQueryValue::String(value.to_string()),
            negated,
        )]),
        _ if normalized_prefix == "is" => is_node(value, negated),
        _ if normalized_prefix == "has" => has_node(value, negated),
        _ if matches!(normalized_prefix.as_str(), "tag" | "keyword") => Ok(vec![condition_node(
            MailQueryField::Keyword,
            MailQueryOperator::Equals,
            MailQueryValue::String(value.to_string()),
            negated,
        )]),
        _ if matches!(normalized_prefix.as_str(), "in" | "mailbox") => {
            Ok(vec![mailbox_node(value, negated)])
        }
        _ if matches!(normalized_prefix.as_str(), "source" | "account") => {
            Ok(vec![source_node(value, negated)])
        }
        _ if normalized_prefix == "id" => Ok(vec![condition_node(
            MailQueryField::MessageId,
            MailQueryOperator::Equals,
            MailQueryValue::String(value.to_string()),
            negated,
        )]),
        _ if matches!(normalized_prefix.as_str(), "thread" | "threadid") => {
            Ok(vec![condition_node(
                MailQueryField::ThreadId,
                MailQueryOperator::Equals,
                MailQueryValue::String(value.to_string()),
                negated,
            )])
        }
        _ if matches!(
            normalized_prefix.as_str(),
            "conversation" | "conversationid" | "conv"
        ) =>
        {
            Ok(vec![condition_node(
                MailQueryField::ConversationId,
                MailQueryOperator::Equals,
                MailQueryValue::String(value.to_string()),
                negated,
            )])
        }
        _ if normalized_prefix == "before" => Ok(vec![condition_node(
            MailQueryField::ReceivedAt,
            MailQueryOperator::Lt,
            MailQueryValue::String(value.to_string()),
            negated,
        )]),
        _ if normalized_prefix == "after" => Ok(vec![condition_node(
            MailQueryField::ReceivedAt,
            MailQueryOperator::Gt,
            MailQueryValue::String(value.to_string()),
            negated,
        )]),
        _ if normalized_prefix == "date" => date_node(value, negated),
        _ if normalized_prefix == "newer" => {
            relative_date_node(value, MailQueryOperator::Gt, negated)
        }
        _ if normalized_prefix == "older" => {
            relative_date_node(value, MailQueryOperator::Lt, negated)
        }
        _ => Err(format!("unknown search prefix: {prefix}")),
    }
}

// -- helpers ----------------------------------------------------------------

pub(super) fn condition_node(
    field: MailQueryField,
    operator: MailQueryOperator,
    value: MailQueryValue,
    negated: bool,
) -> MailQueryRuleNode {
    MailQueryRuleNode::Condition(MailQueryCondition {
        field,
        operator,
        negated,
        value,
    })
}

/// `from:value` -> ANY(FromEmail contains, FromName contains)
fn from_node(value: &str, negated: bool) -> MailQueryRuleNode {
    MailQueryRuleNode::Group(MailQueryGroup {
        operator: MailQueryGroupOperator::Any,
        negated,
        nodes: vec![
            MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::FromEmail,
                operator: MailQueryOperator::Contains,
                negated: false,
                value: MailQueryValue::String(value.to_string()),
            }),
            MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::FromName,
                operator: MailQueryOperator::Contains,
                negated: false,
                value: MailQueryValue::String(value.to_string()),
            }),
        ],
    })
}

/// `in:value` -> ANY(mailbox role exact, mailbox id exact, mailbox name contains)
fn mailbox_node(value: &str, negated: bool) -> MailQueryRuleNode {
    MailQueryRuleNode::Group(MailQueryGroup {
        operator: MailQueryGroupOperator::Any,
        negated,
        nodes: vec![
            MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::MailboxRole,
                operator: MailQueryOperator::Equals,
                negated: false,
                value: MailQueryValue::String(value.to_string()),
            }),
            MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::MailboxId,
                operator: MailQueryOperator::Equals,
                negated: false,
                value: MailQueryValue::String(value.to_string()),
            }),
            MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::MailboxName,
                operator: MailQueryOperator::Contains,
                negated: false,
                value: MailQueryValue::String(value.to_string()),
            }),
        ],
    })
}

/// `source:value` -> ANY(source id exact, source display name contains)
fn source_node(value: &str, negated: bool) -> MailQueryRuleNode {
    MailQueryRuleNode::Group(MailQueryGroup {
        operator: MailQueryGroupOperator::Any,
        negated,
        nodes: vec![
            MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::SourceId,
                operator: MailQueryOperator::Equals,
                negated: false,
                value: MailQueryValue::String(value.to_string()),
            }),
            MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::SourceName,
                operator: MailQueryOperator::Contains,
                negated: false,
                value: MailQueryValue::String(value.to_string()),
            }),
        ],
    })
}

/// `is:unread` / `is:flagged`
fn is_node(value: &str, negated: bool) -> Result<Vec<MailQueryRuleNode>, String> {
    let value = value.to_ascii_lowercase();
    match value.as_str() {
        "unread" => Ok(vec![condition_node(
            MailQueryField::IsRead,
            MailQueryOperator::Equals,
            MailQueryValue::Bool(false),
            negated,
        )]),
        "read" | "seen" => Ok(vec![condition_node(
            MailQueryField::IsRead,
            MailQueryOperator::Equals,
            MailQueryValue::Bool(true),
            negated,
        )]),
        "flagged" => Ok(vec![condition_node(
            MailQueryField::IsFlagged,
            MailQueryOperator::Equals,
            MailQueryValue::Bool(true),
            negated,
        )]),
        "unflagged" => Ok(vec![condition_node(
            MailQueryField::IsFlagged,
            MailQueryOperator::Equals,
            MailQueryValue::Bool(false),
            negated,
        )]),
        "attachment" | "attachments" => Ok(vec![condition_node(
            MailQueryField::HasAttachment,
            MailQueryOperator::Equals,
            MailQueryValue::Bool(true),
            negated,
        )]),
        _ => Err(format!("unknown is: value: {value}")),
    }
}

/// `has:attachment`
fn has_node(value: &str, negated: bool) -> Result<Vec<MailQueryRuleNode>, String> {
    let value = value.to_ascii_lowercase();
    match value.as_str() {
        "attachment" | "attachments" => Ok(vec![condition_node(
            MailQueryField::HasAttachment,
            MailQueryOperator::Equals,
            MailQueryValue::Bool(true),
            negated,
        )]),
        _ => Err(format!("unknown has: value: {value}")),
    }
}

/// Free text: search across FromName, FromEmail, Subject, Preview.
fn free_text_node(value: &str, negated: bool) -> MailQueryRuleNode {
    MailQueryRuleNode::Group(MailQueryGroup {
        operator: MailQueryGroupOperator::Any,
        negated,
        nodes: vec![
            MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::FromName,
                operator: MailQueryOperator::Contains,
                negated: false,
                value: MailQueryValue::String(value.to_string()),
            }),
            MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::FromEmail,
                operator: MailQueryOperator::Contains,
                negated: false,
                value: MailQueryValue::String(value.to_string()),
            }),
            MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::Subject,
                operator: MailQueryOperator::Contains,
                negated: false,
                value: MailQueryValue::String(value.to_string()),
            }),
            MailQueryRuleNode::Condition(MailQueryCondition {
                field: MailQueryField::Preview,
                operator: MailQueryOperator::Contains,
                negated: false,
                value: MailQueryValue::String(value.to_string()),
            }),
        ],
    })
}
