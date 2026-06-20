use super::*;

/// Mailbox read projection for synced account navigation.
pub trait MailboxReadStore: Send + Sync {
    /// List all mailboxes for an account.
    ///
    /// @spec docs/L1-sync#sqlite-schema
    fn list_mailboxes(&self, account_id: &AccountId) -> Result<Vec<MailboxSummary>, StoreError>;
}

/// Local mailbox metadata overrides for provider fields that cannot be mutated
/// remotely by every driver.
pub trait MailboxRoleOverrideStore: Send + Sync {
    /// Store a local mailbox role override.
    ///
    /// `role = None` is an explicit local clear, not "remove the override".
    fn set_mailbox_role_override(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
        role: Option<&str>,
        clear_role_from: Option<&MailboxId>,
    ) -> Result<(), StoreError>;
}

/// Message list projection for UI queries.
pub trait MessageListStore: Send + Sync {
    /// List messages, optionally filtered by mailbox.
    ///
    /// @spec docs/L1-sync#sqlite-schema
    fn list_messages(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, StoreError>;

    /// Paginated message list with seek-based cursors.
    ///
    /// @spec docs/L1-api#cursor-pagination
    fn list_message_page(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, StoreError>;
}

/// Tag read projection for non-system JMAP keywords.
pub trait TagReadStore: Send + Sync {
    /// List user-facing tags for one account with unread and total counts.
    ///
    /// @spec docs/L1-sync#sqlite-schema
    fn list_tags(&self, account_id: &AccountId) -> Result<Vec<TagSummary>, StoreError>;
}

/// Conversation list and detail projection for UI queries.
pub trait ConversationReadStore: Send + Sync {
    /// Paginated conversation list with seek-based cursors.
    ///
    /// @spec docs/L1-sync#conversation-pagination
    fn list_conversations(
        &self,
        account_id: Option<&AccountId>,
        mailbox_id: Option<&MailboxId>,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, StoreError>;

    /// Fetch a single conversation with all its messages.
    ///
    /// @spec docs/L1-sync#conversation-pagination
    fn get_conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<Option<ConversationView>, StoreError>;
}

/// Message detail read projection for message views and thread views.
pub trait MessageDetailStore: Send + Sync {
    /// Fetch full message detail including body content.
    ///
    /// @spec docs/L1-sync#body-lazy
    fn get_message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, StoreError>;

    /// Read the cached raw RFC822 bytes for a message, if any are stored.
    ///
    /// Returns `None` when no raw body has been cached yet. Used to serve
    /// attachment bytes from a previously fetched message without re-fetching
    /// it from the provider.
    ///
    /// @spec docs/L1-sync#body-lazy
    fn read_raw_message(
        &self,
        _account_id: &AccountId,
        _message_id: &MessageId,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(None)
    }

    /// Fetch all messages in a thread.
    ///
    /// @spec docs/L1-sync#sqlite-schema
    fn get_thread(
        &self,
        account_id: &AccountId,
        thread_id: &ThreadId,
    ) -> Result<Option<ThreadView>, StoreError>;
}

/// Smart mailbox rule evaluation over synced mail projections.
pub trait SmartMailboxStore: Send + Sync {
    /// Query messages matching a smart mailbox rule.
    ///
    /// @spec docs/L0-search#smart-mailboxes
    fn query_messages_by_rule(
        &self,
        rule: &SmartMailboxRule,
    ) -> Result<Vec<MessageSummary>, StoreError>;

    /// Query messages matching a smart mailbox rule with explicit ordering.
    fn query_messages_by_rule_sorted(
        &self,
        rule: &SmartMailboxRule,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<Vec<MessageSummary>, StoreError>;

    /// Query messages matching a smart mailbox rule with seek pagination.
    ///
    /// @spec docs/L1-api#cursor-pagination
    fn query_message_page_by_rule(
        &self,
        rule: &SmartMailboxRule,
        limit: usize,
        cursor: Option<&MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    ) -> Result<MessagePage, StoreError>;

    /// Query conversations matching a smart mailbox rule with pagination.
    ///
    /// @spec docs/L1-sync#conversation-pagination
    fn query_conversations_by_rule(
        &self,
        rule: &SmartMailboxRule,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, StoreError>;

    /// Return (unread, total) counts for a smart mailbox rule.
    ///
    /// @spec docs/L1-search#smart-mailbox-data-model
    fn query_smart_mailbox_counts(&self, rule: &SmartMailboxRule)
        -> Result<(i64, i64), StoreError>;
}
