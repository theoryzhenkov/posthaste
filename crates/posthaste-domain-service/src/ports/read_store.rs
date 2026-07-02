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

    /// Fetch a message's summary (header-level projection) WITHOUT its body or
    /// attachments. Callers that need only metadata — mailbox membership,
    /// keywords, existence — must use this rather than [`Self::get_message_detail`]
    /// so a keyword/mailbox/destroy path never materializes the body. (Loading
    /// the body for metadata is the slowdown that
    /// `message_mutation_settlement_payload_excludes_the_message_body` guards.)
    ///
    /// The default derives it from `get_message_detail`; a store overrides it to
    /// skip the body/attachment reads entirely.
    fn get_message_summary(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageSummary>, StoreError> {
        Ok(self
            .get_message_detail(account_id, message_id)?
            .map(|detail| detail.summary))
    }

    /// Fetch message detail WITHOUT the body (header + attachments). The body is
    /// a separate lazy resource (`GET .../body`), so the detail read surface must
    /// not load it — loading the body only to drop it from the detail response is
    /// wasted work, dominant for messages with large bodies/attachments.
    ///
    /// The default derives it from `get_message_detail` (nulling the body); a
    /// store overrides it to skip the body query entirely.
    fn get_message_detail_without_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, StoreError> {
        Ok(self
            .get_message_detail(account_id, message_id)?
            .map(|mut detail| {
                detail.body_html = None;
                detail.body_text = None;
                detail.raw_message = None;
                detail
            }))
    }

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

/// Phase 2 undo/redo: the per-account reversible-op log + cursor read. Serves
/// the `RevLog` synced view. The `diff` is opaque `MessageChangeDiff` JSON.
///
/// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
pub trait RevLogStore: Send + Sync {
    /// The account's `rev_log` steps + cursor — the snapshot behind the `RevLog`
    /// synced view. Eviction keeps the log bounded (`MAX_REV_LOG_HISTORY`), so
    /// this returns the full undoable range.
    fn rev_log_snapshot(&self, account_id: &AccountId) -> Result<RevLogSnapshot, StoreError>;

    /// Append a reversible-op step (Phase 2 forward-action confirm). Idempotent
    /// on `step_id`; assigns `seq = MAX(seq) + 1`. `diff` is the opaque
    /// `MessageChangeDiff` JSON captured client-side.
    ///
    /// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
    fn append_rev_log_step(
        &self,
        account_id: &AccountId,
        step_id: &str,
        message_id: &str,
        source_id: &str,
        diff: &serde_json::Value,
        created_at: &str,
    ) -> Result<u32, StoreError>;

    /// Set the account's undo/redo cursor (idempotent upsert).
    /// `cursor_step_id = None` means all undone. The caller (server) validates
    /// the referenced steps exist before calling this.
    ///
    /// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
    fn set_rev_cursor(
        &self,
        account_id: &AccountId,
        cursor_step_id: Option<&str>,
        redo_tail: &[String],
    ) -> Result<(), StoreError>;
}

/// Snooze state: a Posthaste-local return time for a message in the Snoozed
/// mailbox. Not provider-synced. The scheduler (supervisor snooze tick) scans
/// `list_due_snoozes` for due rows + auto-returns them.
///
/// @spec docs/eph/DESIGN-L2-snooze
pub trait SnoozeStore: Send + Sync {
    /// Insert (or replace) a snooze row for a message. Called by `message.snooze`
    /// after the move to the Snoozed mailbox.
    fn insert_snooze(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        until: i64,
    ) -> Result<(), StoreError>;

    /// Delete a message's snooze row. Called by `message.unsnooze` + the scheduler
    /// auto-return. (The store invariant in `replace_mailboxes_tx` also deletes
    /// the row whenever a message leaves the Snoozed mailbox, so this is
    /// idempotent.)
    fn delete_snooze(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), StoreError>;

    /// Messages whose snooze return time has arrived (`until <= now`), for the
    /// scheduler tick.
    fn list_due_snoozes(
        &self,
        account_id: &AccountId,
        now: i64,
    ) -> Result<Vec<(MessageId, i64)>, StoreError>;
}
