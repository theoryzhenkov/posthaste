//! The mail-list family: windowed list evaluation, the filter-to-AST
//! compiler, and the opaque cursor codec.

use posthaste_client_models::{MailListQuery, MailListResult};
use posthaste_domain_model::{
    AccountId, MailQueryCondition, MailQueryField, MailQueryGroup, MailQueryGroupOperator,
    MailQueryOperator, MailQueryRule, MailQueryRuleNode, MailQueryValue, MessageCursor, MessageId,
    SortDirection,
};

use super::{ApiFailure, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT};
use crate::AppState;

pub(crate) fn evaluate_mail_list(
    app: &AppState,
    query: MailListQuery,
) -> Result<MailListResult, ApiFailure> {
    let limit = query
        .limit
        .map(|limit| limit as usize)
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .clamp(1, MAX_LIST_LIMIT);
    let cursor = query
        .cursor
        .as_deref()
        .map(parse_message_cursor)
        .transpose()?;
    let sort = query.sort.clone().unwrap_or_default();
    let direction = if sort.descending {
        SortDirection::Desc
    } else {
        SortDirection::Asc
    };
    let rule = match &query.smart_mailbox_id {
        Some(smart_mailbox_id) => {
            if query.mailbox_id.is_some() {
                return Err(ApiFailure::malformed(
                    "mailboxId and smartMailboxId are mutually exclusive scopes",
                ));
            }
            // The saved rule scopes the list; the remaining filters AND on
            // top of it.
            let smart_mailbox = app.service.get_smart_mailbox(smart_mailbox_id)?;
            let mut root = mail_list_rule(&query).root;
            root.nodes
                .insert(0, MailQueryRuleNode::Group(smart_mailbox.rule.root));
            MailQueryRule { root }
        }
        None => mail_list_rule(&query),
    };
    let page = app.service.query_message_page_by_rule(
        &rule,
        limit,
        cursor.as_ref(),
        sort.field,
        direction,
    )?;
    Ok(MailListResult {
        rows: page.items,
        next_cursor: page.next_cursor.as_ref().map(encode_message_cursor),
    })
}

/// Compile the mail-list filters into the shared mail-query AST: scope and
/// flag filters AND together; free text is an OR group over subject, sender
/// name/email, recipients, preview, and the cached body index.
fn mail_list_rule(query: &MailListQuery) -> MailQueryRule {
    fn condition(field: MailQueryField, value: MailQueryValue) -> MailQueryRuleNode {
        MailQueryRuleNode::Condition(MailQueryCondition {
            field,
            operator: MailQueryOperator::Equals,
            negated: false,
            value,
        })
    }

    let mut nodes = Vec::new();
    if let Some(account_id) = &query.account_id {
        nodes.push(condition(
            MailQueryField::SourceId,
            MailQueryValue::String(account_id.to_string()),
        ));
    }
    if let Some(mailbox_id) = &query.mailbox_id {
        nodes.push(condition(
            MailQueryField::MailboxId,
            MailQueryValue::String(mailbox_id.to_string()),
        ));
    }
    if let Some(is_read) = query.is_read {
        nodes.push(condition(
            MailQueryField::IsRead,
            MailQueryValue::Bool(is_read),
        ));
    }
    if let Some(is_flagged) = query.is_flagged {
        nodes.push(condition(
            MailQueryField::IsFlagged,
            MailQueryValue::Bool(is_flagged),
        ));
    }
    if let Some(has_attachment) = query.has_attachment {
        nodes.push(condition(
            MailQueryField::HasAttachment,
            MailQueryValue::Bool(has_attachment),
        ));
    }
    if let Some(text) = query
        .free_text
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        let contains = |field| {
            MailQueryRuleNode::Condition(MailQueryCondition {
                field,
                operator: MailQueryOperator::Contains,
                negated: false,
                value: MailQueryValue::String(text.to_string()),
            })
        };
        nodes.push(MailQueryRuleNode::Group(MailQueryGroup {
            operator: MailQueryGroupOperator::Any,
            negated: false,
            nodes: vec![
                contains(MailQueryField::Subject),
                contains(MailQueryField::FromName),
                contains(MailQueryField::FromEmail),
                contains(MailQueryField::To),
                contains(MailQueryField::Preview),
                contains(MailQueryField::Body),
            ],
        }));
    }
    MailQueryRule {
        root: MailQueryGroup {
            operator: MailQueryGroupOperator::All,
            negated: false,
            nodes,
        },
    }
}

/// Opaque mail-list cursor codec:
/// `{sort_len}:{sort_value}:{source_len}:{source_id}:{message_id}`.
fn encode_message_cursor(cursor: &MessageCursor) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        cursor.sort_value.len(),
        cursor.sort_value,
        cursor.source_id.as_str().len(),
        cursor.source_id.as_str(),
        cursor.message_id.as_str()
    )
}

fn parse_message_cursor(cursor: &str) -> Result<MessageCursor, ApiFailure> {
    fn take_prefixed(value: &str) -> Option<(&str, &str)> {
        let (len_prefix, remainder) = value.split_once(':')?;
        let value_len = len_prefix.parse::<usize>().ok()?;
        // The length is client-supplied bytes: reject it unless it lands on
        // a char boundary of the remainder (`split_at` panics otherwise —
        // sort values legitimately carry multi-byte UTF-8).
        if remainder.len() <= value_len || !remainder.is_char_boundary(value_len) {
            return None;
        }
        let (prefixed, remainder) = remainder.split_at(value_len);
        Some((prefixed, remainder.strip_prefix(':')?))
    }

    let invalid = || ApiFailure::malformed("malformed mail-list cursor");
    let (sort_value, remainder) = take_prefixed(cursor).ok_or_else(invalid)?;
    let (source_id, message_id) = take_prefixed(remainder).ok_or_else(invalid)?;
    if source_id.is_empty() || message_id.is_empty() {
        return Err(invalid());
    }
    Ok(MessageCursor {
        sort_value: sort_value.to_string(),
        source_id: AccountId::from(source_id),
        message_id: MessageId::from(message_id),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_cursor_round_trips_multibyte_sort_values() {
        let cursor = MessageCursor {
            sort_value: "Résumé — überprüfen".to_string(),
            source_id: AccountId::from("acct-1"),
            message_id: MessageId::from("msg-1"),
        };
        let parsed = parse_message_cursor(&encode_message_cursor(&cursor)).expect("parses back");
        assert_eq!(parsed.sort_value, cursor.sort_value);
        assert_eq!(parsed.source_id.as_str(), "acct-1");
        assert_eq!(parsed.message_id.as_str(), "msg-1");
    }

    #[test]
    fn malformed_cursors_are_rejected_without_panicking() {
        for cursor in [
            "",
            "no-len",
            "9:short",
            "1:\u{e9}:1:a:b", // the length lands inside a multi-byte char
            "1:x:999:a:b",
            "0::0::",
        ] {
            assert!(parse_message_cursor(cursor).is_err(), "cursor {cursor:?}");
        }
    }
}
