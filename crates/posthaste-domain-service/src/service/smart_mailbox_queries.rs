use super::*;

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
            let (unread, total) = self.count_messages_by_rule(&mailbox.rule)?;
            summaries.push(SmartMailboxSummary {
                id: mailbox.id,
                name: mailbox.name,
                kind: mailbox.kind,
                default_key: mailbox.default_key,
                role: mailbox.role,
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
        rule: &MailQueryRule,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        // Indexed SQL over canonical. Optimism is written through to canonical
        // (S2), so the store's rule query serves the optimistic state directly
        // — no load-all + in-memory fold/filter (the dropped O(all) regression).
        self.smart_mailboxes
            .query_messages_by_rule(rule)
            .map_err(ServiceError::from)
    }

    /// Count messages matching an explicit smart mailbox rule.
    ///
    /// @spec docs/L1-search#execution-pipeline
    pub fn count_messages_by_rule(&self, rule: &MailQueryRule) -> Result<(i64, i64), ServiceError> {
        // Indexed SQL counts over canonical (optimism is written through, S2).
        self.smart_mailboxes
            .query_smart_mailbox_counts(rule)
            .map_err(ServiceError::from)
    }

    /// Messages matching an explicit smart mailbox rule with explicit ordering.
    ///
    /// @spec docs/L1-search#execution-pipeline
    pub fn query_messages_by_rule_sorted(
        &self,
        rule: &MailQueryRule,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<Vec<MessageSummary>, ServiceError> {
        self.smart_mailboxes
            .query_messages_by_rule_sorted(rule, sort_field, sort_direction)
            .map_err(ServiceError::from)
    }

    /// Paginated messages matching an explicit smart mailbox rule.
    ///
    /// @spec docs/L1-search#execution-pipeline
    pub fn query_message_page_by_rule(
        &self,
        rule: &MailQueryRule,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, ServiceError> {
        // The runtime view-recompute hot path: indexed SQL `ORDER BY … LIMIT`
        // with seek pagination over canonical (optimism written through, S2).
        // Was load-all + in-memory sort + skip/take — the O(all) regression.
        self.smart_mailboxes
            .query_message_page_by_rule(rule, limit, cursor, sort_field, sort_direction)
            .map_err(ServiceError::from)
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
        rule: &MailQueryRule,
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
