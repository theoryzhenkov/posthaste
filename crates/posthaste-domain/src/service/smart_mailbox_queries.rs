use super::message_queries::{message_cursor, sort_message_summaries};
use super::*;
use crate::{
    SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
    SmartMailboxOperator, SmartMailboxRuleNode, SmartMailboxValue,
};

impl MailService {
    /// Find a saved query by id, default key, or case-insensitive name.
    pub fn find_smart_mailbox(&self, selector: &str) -> Result<Option<SmartMailbox>, ServiceError> {
        let selector = selector.trim();
        if selector.is_empty() {
            return Ok(None);
        }
        let id = SmartMailboxId::from(selector);
        if let Some(mailbox) = self.config.get_smart_mailbox(&id)? {
            return Ok(Some(mailbox));
        }
        let normalized = selector.to_ascii_lowercase();
        Ok(self
            .config
            .list_smart_mailboxes()?
            .into_iter()
            .find(|mailbox| {
                mailbox.name.eq_ignore_ascii_case(selector)
                    || mailbox
                        .default_key
                        .as_deref()
                        .is_some_and(|key| key.eq_ignore_ascii_case(&normalized))
            }))
    }

    /// List smart mailboxes with live unread/total counts from the store.
    ///
    /// @spec docs/L1-api#smart-mailboxes
    pub fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailboxSummary>, ServiceError> {
        let mailboxes = self.config.list_smart_mailboxes()?;
        let mut summaries = Vec::with_capacity(mailboxes.len());
        for mailbox in mailboxes {
            // Lazy unoptimized counts (folded over the overlay); see
            // `count_messages_by_rule`.
            let (unread, total) = self.folded_rule_counts(&mailbox.rule)?;
            summaries.push(SmartMailboxSummary {
                id: mailbox.id,
                name: mailbox.name,
                position: mailbox.position,
                kind: mailbox.kind,
                default_key: mailbox.default_key,
                parent_id: mailbox.parent_id,
                unread_messages: unread,
                total_messages: total,
                created_at: mailbox.created_at,
                updated_at: mailbox.updated_at,
            });
        }
        Ok(summaries)
    }

    /// List messages matching a smart mailbox's rule.
    ///
    /// @spec docs/L1-api#smart-mailboxes
    pub fn list_smart_mailbox_messages(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        let mailbox = self
            .config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())?;
        self.query_messages_by_rule(&mailbox.rule)
    }

    /// Paginated messages matching a smart mailbox's rule.
    ///
    /// @spec docs/L1-api#smart-mailboxes
    pub fn list_smart_mailbox_message_page(
        &self,
        smart_mailbox_id: &SmartMailboxId,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, ServiceError> {
        let mailbox = self
            .config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())?;
        self.query_message_page_by_rule(&mailbox.rule, limit, cursor, sort_field, sort_direction)
    }

    /// List messages matching an explicit smart mailbox rule.
    ///
    /// @spec docs/L1-search#execution-pipeline
    pub fn query_messages_by_rule(
        &self,
        rule: &SmartMailboxRule,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        let mut folded = Vec::new();
        for account in self.config.list_sources()? {
            let summaries = self
                .message_lister
                .list_messages(&account.id, None)
                .map_err(ServiceError::from)?;
            let mailboxes = self.mailbox_reader.list_mailboxes(&account.id)?;
            folded.extend(
                self.fold_message_overlay(&account.id, summaries, None)?
                    .into_iter()
                    .filter(|summary| rule_matches_summary(rule, summary, &mailboxes)),
            );
        }
        Ok(folded)
    }

    /// Count messages matching an explicit smart mailbox rule.
    ///
    /// @spec docs/L1-search#execution-pipeline
    pub fn count_messages_by_rule(
        &self,
        rule: &SmartMailboxRule,
    ) -> Result<(i64, i64), ServiceError> {
        // Counts are computed lazily by folding the overlay over matching
        // messages — unoptimized and always one path (the SQL fast-path and the
        // any-pending gate are gone). Precise counts are rarely needed;
        // incremental materialized counters (updated per operation, accounting
        // for smart-mailbox membership and actions) are a deliberate later
        // refinement.
        //
        // @spec docs/replication/L1#derived-not-replicated
        self.folded_rule_counts(rule)
    }

    /// `(unread, total)` for a rule computed over the read-time overlay.
    fn folded_rule_counts(&self, rule: &SmartMailboxRule) -> Result<(i64, i64), ServiceError> {
        let messages = self.query_messages_by_rule(rule)?;
        let total = messages.len() as i64;
        let unread = messages.iter().filter(|message| !message.is_read).count() as i64;
        Ok((unread, total))
    }

    /// Messages matching an explicit smart mailbox rule with explicit ordering.
    ///
    /// @spec docs/L1-search#execution-pipeline
    pub fn query_messages_by_rule_sorted(
        &self,
        rule: &SmartMailboxRule,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        let mut items = self.query_messages_by_rule(rule)?;
        sort_message_summaries(&mut items, sort_field, sort_direction);
        Ok(items)
    }

    /// Paginated messages matching an explicit smart mailbox rule.
    ///
    /// @spec docs/L1-search#execution-pipeline
    pub fn query_message_page_by_rule(
        &self,
        rule: &SmartMailboxRule,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, ServiceError> {
        let mut items = self.query_messages_by_rule(rule)?;
        sort_message_summaries(&mut items, sort_field, sort_direction);
        let start = cursor
            .and_then(|cursor| {
                items.iter().position(|item| {
                    item.source_id == cursor.source_id && item.id == cursor.message_id
                })
            })
            .map_or(0, |index| index + 1);
        let page_items = items
            .iter()
            .skip(start)
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        let next_cursor = if start + page_items.len() < items.len() {
            page_items
                .last()
                .map(|item| message_cursor(item, sort_field))
        } else {
            None
        };
        Ok(MessagePage {
            items: page_items,
            next_cursor,
        })
    }

    /// Paginated conversations matching a smart mailbox's rule.
    ///
    /// @spec docs/L1-api#smart-mailboxes
    pub fn list_smart_mailbox_conversations(
        &self,
        smart_mailbox_id: &SmartMailboxId,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, ServiceError> {
        let mailbox = self
            .config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())?;
        self.smart_mailboxes
            .query_conversations_by_rule(&mailbox.rule, limit, cursor, sort_field, sort_direction)
            .map_err(Into::into)
    }

    /// Query conversations matching an arbitrary rule (used by search).
    pub fn query_conversations_by_rule(
        &self,
        rule: &SmartMailboxRule,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, ServiceError> {
        self.smart_mailboxes
            .query_conversations_by_rule(rule, limit, cursor, sort_field, sort_direction)
            .map_err(Into::into)
    }

    // -- Store delegates (runtime data) --
}
fn rule_matches_summary(
    rule: &SmartMailboxRule,
    summary: &MessageSummary,
    mailboxes: &[MailboxSummary],
) -> bool {
    group_matches_summary(&rule.root, summary, mailboxes)
}

fn group_matches_summary(
    group: &SmartMailboxGroup,
    summary: &MessageSummary,
    mailboxes: &[MailboxSummary],
) -> bool {
    let matches = match group.operator {
        SmartMailboxGroupOperator::All => group
            .nodes
            .iter()
            .all(|node| node_matches_summary(node, summary, mailboxes)),
        SmartMailboxGroupOperator::Any => group
            .nodes
            .iter()
            .any(|node| node_matches_summary(node, summary, mailboxes)),
    };
    matches ^ group.negated
}

fn node_matches_summary(
    node: &SmartMailboxRuleNode,
    summary: &MessageSummary,
    mailboxes: &[MailboxSummary],
) -> bool {
    match node {
        SmartMailboxRuleNode::Group(group) => group_matches_summary(group, summary, mailboxes),
        SmartMailboxRuleNode::Condition(condition) => {
            condition_matches_summary(condition, summary, mailboxes) ^ condition.negated
        }
    }
}

fn condition_matches_summary(
    condition: &SmartMailboxCondition,
    summary: &MessageSummary,
    mailboxes: &[MailboxSummary],
) -> bool {
    match condition.field {
        SmartMailboxField::SourceId => match_text(condition, &[summary.source_id.as_str()]),
        SmartMailboxField::SourceName => match_text(condition, &[summary.source_name.as_str()]),
        SmartMailboxField::MessageId => match_text(condition, &[summary.id.as_str()]),
        SmartMailboxField::ThreadId => match_text(condition, &[summary.source_thread_id.as_str()]),
        SmartMailboxField::ConversationId => {
            match_text(condition, &[summary.conversation_id.as_str()])
        }
        SmartMailboxField::MailboxId => match_text(
            condition,
            &summary
                .mailbox_ids
                .iter()
                .map(MailboxId::as_str)
                .collect::<Vec<_>>(),
        ),
        SmartMailboxField::IsRead => match_bool(condition, summary.is_read),
        SmartMailboxField::IsFlagged => match_bool(condition, summary.is_flagged),
        SmartMailboxField::HasAttachment => match_bool(condition, summary.has_attachment),
        SmartMailboxField::Keyword => match_text(
            condition,
            &summary
                .keywords
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        ),
        SmartMailboxField::FromName => match_text(
            condition,
            optional_text(summary.from_name.as_deref()).as_slice(),
        ),
        SmartMailboxField::FromEmail => match_text(
            condition,
            optional_text(summary.from_email.as_deref()).as_slice(),
        ),
        SmartMailboxField::Subject => match_text(
            condition,
            optional_text(summary.subject.as_deref()).as_slice(),
        ),
        SmartMailboxField::Preview => match_text(
            condition,
            optional_text(summary.preview.as_deref()).as_slice(),
        ),
        SmartMailboxField::ReceivedAt => match_text(condition, &[summary.received_at.as_str()]),
        SmartMailboxField::MailboxName => {
            match_mailbox_text(condition, summary, mailboxes, |mailbox| {
                Some(mailbox.name.as_str())
            })
        }
        SmartMailboxField::MailboxRole => {
            match_mailbox_text(condition, summary, mailboxes, |mailbox| {
                mailbox.role.as_deref()
            })
        }
    }
}

fn optional_text(value: Option<&str>) -> Vec<&str> {
    value.into_iter().collect()
}

fn match_mailbox_text<'a>(
    condition: &SmartMailboxCondition,
    summary: &MessageSummary,
    mailboxes: &'a [MailboxSummary],
    value: impl Fn(&'a MailboxSummary) -> Option<&'a str>,
) -> bool {
    let actual_values = mailboxes
        .iter()
        .filter(|mailbox| summary.mailbox_ids.iter().any(|id| id == &mailbox.id))
        .filter_map(value)
        .collect::<Vec<_>>();
    match_text(condition, &actual_values)
}

fn match_bool(condition: &SmartMailboxCondition, actual: bool) -> bool {
    match &condition.value {
        SmartMailboxValue::Bool(expected) => {
            matches!(condition.operator, SmartMailboxOperator::Equals) && actual == *expected
        }
        SmartMailboxValue::String(expected) => {
            matches!(condition.operator, SmartMailboxOperator::Equals)
                && actual.to_string().eq_ignore_ascii_case(expected)
        }
        SmartMailboxValue::Strings(expected) => {
            matches!(condition.operator, SmartMailboxOperator::In)
                && expected
                    .iter()
                    .any(|value| actual.to_string().eq_ignore_ascii_case(value))
        }
    }
}

fn match_text(condition: &SmartMailboxCondition, actual_values: &[&str]) -> bool {
    match condition.operator {
        SmartMailboxOperator::Equals => {
            expected_strings(&condition.value)
                .into_iter()
                .any(|expected| {
                    actual_values
                        .iter()
                        .any(|actual| actual.eq_ignore_ascii_case(&expected))
                })
        }
        SmartMailboxOperator::In => {
            expected_strings(&condition.value)
                .into_iter()
                .any(|expected| {
                    actual_values
                        .iter()
                        .any(|actual| actual.eq_ignore_ascii_case(&expected))
                })
        }
        SmartMailboxOperator::Contains => {
            expected_strings(&condition.value)
                .into_iter()
                .any(|expected| {
                    actual_values.iter().any(|actual| {
                        actual
                            .to_ascii_lowercase()
                            .contains(&expected.to_ascii_lowercase())
                    })
                })
        }
        SmartMailboxOperator::Before => {
            expected_strings(&condition.value)
                .into_iter()
                .any(|expected| {
                    actual_values
                        .iter()
                        .any(|actual| *actual < expected.as_str())
                })
        }
        SmartMailboxOperator::After => {
            expected_strings(&condition.value)
                .into_iter()
                .any(|expected| {
                    actual_values
                        .iter()
                        .any(|actual| *actual > expected.as_str())
                })
        }
        SmartMailboxOperator::OnOrBefore => {
            expected_strings(&condition.value)
                .into_iter()
                .any(|expected| {
                    actual_values
                        .iter()
                        .any(|actual| *actual <= expected.as_str())
                })
        }
        SmartMailboxOperator::OnOrAfter => {
            expected_strings(&condition.value)
                .into_iter()
                .any(|expected| {
                    actual_values
                        .iter()
                        .any(|actual| *actual >= expected.as_str())
                })
        }
    }
}

fn expected_strings(value: &SmartMailboxValue) -> Vec<String> {
    match value {
        SmartMailboxValue::String(value) => vec![value.clone()],
        SmartMailboxValue::Strings(values) => values.clone(),
        SmartMailboxValue::Bool(value) => vec![value.to_string()],
    }
}
