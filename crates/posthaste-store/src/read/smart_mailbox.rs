use super::*;

impl SmartMailboxStore for DatabaseStore {
    /// Evaluates a smart mailbox rule against all sources and returns matching
    /// messages.
    ///
    /// @spec docs/L1-search#smart-mailbox-data-model
    fn query_messages_by_rule(
        &self,
        rule: &MailQueryRule,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        let connection = self.read_connection()?;
        query_messages_by_rule(&connection, rule)
    }

    /// Evaluates a smart mailbox rule and returns all matching messages with explicit ordering.
    fn query_messages_by_rule_sorted(
        &self,
        rule: &MailQueryRule,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        let connection = self.read_connection()?;
        query_messages_by_rule_sorted(&connection, rule, sort_field, sort_direction)
    }

    /// Evaluates a smart mailbox rule and returns a paginated message view.
    ///
    /// @spec docs/L1-api#cursor-pagination
    fn query_message_page_by_rule(
        &self,
        rule: &MailQueryRule,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, StoreError> {
        let connection = self.read_connection()?;
        query_message_page_by_rule(&connection, rule, limit, cursor, sort_field, sort_direction)
    }

    /// Evaluates a smart mailbox rule and returns a paginated conversation view.
    ///
    /// @spec docs/L1-search#smart-mailbox-data-model
    fn query_conversations_by_rule(
        &self,
        rule: &MailQueryRule,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, StoreError> {
        let connection = self.read_connection()?;
        query_conversations_by_rule(&connection, rule, limit, cursor, sort_field, sort_direction)
    }

    /// Returns (unread, total) message counts for a smart mailbox rule.
    fn query_smart_mailbox_counts(&self, rule: &MailQueryRule) -> Result<(i64, i64), StoreError> {
        let connection = self.read_connection()?;
        count_smart_mailbox_messages(&connection, rule)
    }
}
