use super::*;

#[cfg(test)]
pub(super) fn parse_optional_search_rule(
    query: Option<&str>,
) -> Result<Option<SmartMailboxRule>, ApiError> {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return Ok(None);
    };
    posthaste_query_grammar::parse_query(query)
        .map(Some)
        .map_err(|msg| ApiError::new(StatusCode::BAD_REQUEST, ApiErrorCode::InvalidQuery, msg))
}

pub(super) fn account_query(account_id: &AccountId) -> String {
    prefixed_query("in", format!("{}/", account_id.as_str()))
}

pub(super) fn mailbox_query(account_id: &AccountId, mailbox_id: &MailboxId) -> String {
    prefixed_query(
        "in",
        format!("{}/{}", account_id.as_str(), mailbox_id.as_str()),
    )
}

pub(super) fn smart_mailbox_query(smart_mailbox_id: &SmartMailboxId) -> String {
    prefixed_query("in", smart_mailbox_id.as_str())
}

pub(super) fn join_query(parts: impl IntoIterator<Item = Option<String>>) -> String {
    parts
        .into_iter()
        .flatten()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn optional_user_query(query: Option<&str>) -> Option<String> {
    query
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) fn visibility_for_search(
    base_query: String,
    search_query: Option<&str>,
    operation_id: Option<String>,
) -> Option<SearchVisibilityRequest> {
    search_query
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .map(|_| SearchVisibilityRequest {
            base_query,
            operation_id,
        })
}

pub(super) fn expect_message_page(page: MailQueryPage) -> Result<MessagePage, ApiError> {
    match page {
        MailQueryPage::Messages(page) => Ok(page),
        MailQueryPage::CollapsedByConversation(_) => Err(internal_error(
            "runtime returned a conversation page for a message query".to_string(),
        )),
    }
}

pub(super) fn expect_conversation_page(page: MailQueryPage) -> Result<ConversationPage, ApiError> {
    match page {
        MailQueryPage::CollapsedByConversation(page) => Ok(page),
        MailQueryPage::Messages(_) => Err(internal_error(
            "runtime returned a message page for a conversation query".to_string(),
        )),
    }
}

fn prefixed_query(prefix: &str, value: impl AsRef<str>) -> String {
    let value = value.as_ref();
    format!("{prefix}:\"{value}\"")
}

#[cfg(test)]
pub(super) fn source_message_scope_rule(
    source_id: &str,
    mailbox_id: Option<&MailboxId>,
) -> SmartMailboxRule {
    let mut nodes = vec![SmartMailboxRuleNode::Condition(SmartMailboxCondition {
        field: SmartMailboxField::SourceId,
        operator: SmartMailboxOperator::Equals,
        negated: false,
        value: SmartMailboxValue::String(source_id.to_string()),
    })];
    if let Some(mailbox_id) = mailbox_id {
        nodes.push(SmartMailboxRuleNode::Condition(SmartMailboxCondition {
            field: SmartMailboxField::MailboxId,
            operator: SmartMailboxOperator::Equals,
            negated: false,
            value: SmartMailboxValue::String(mailbox_id.as_str().to_string()),
        }));
    }
    SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes,
        },
    }
}
