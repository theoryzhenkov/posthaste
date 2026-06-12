use super::*;

impl MailService {
    /// List smart mailboxes with live unread/total counts from the store.
    ///
    /// @spec docs/L1-api#smart-mailboxes
    pub fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailboxSummary>, ServiceError> {
        let mailboxes = self.config.list_smart_mailboxes()?;
        let mut summaries = Vec::with_capacity(mailboxes.len());
        for mailbox in mailboxes {
            let (unread, total) = self
                .smart_mailboxes
                .query_smart_mailbox_counts(&mailbox.rule)?;
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
        self.smart_mailboxes
            .query_messages_by_rule(&mailbox.rule)
            .map_err(Into::into)
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
        self.smart_mailboxes
            .query_message_page_by_rule(&mailbox.rule, limit, cursor, sort_field, sort_direction)
            .map_err(Into::into)
    }

    /// List messages matching an explicit smart mailbox rule.
    ///
    /// @spec docs/L1-search#execution-pipeline
    pub fn query_messages_by_rule(
        &self,
        rule: &SmartMailboxRule,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        self.smart_mailboxes
            .query_messages_by_rule(rule)
            .map_err(Into::into)
    }

    /// Count messages matching an explicit smart mailbox rule.
    ///
    /// @spec docs/L1-search#execution-pipeline
    pub fn count_messages_by_rule(
        &self,
        rule: &SmartMailboxRule,
    ) -> Result<(i64, i64), ServiceError> {
        self.smart_mailboxes
            .query_smart_mailbox_counts(rule)
            .map_err(Into::into)
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
        self.smart_mailboxes
            .query_message_page_by_rule(rule, limit, cursor, sort_field, sort_direction)
            .map_err(Into::into)
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
