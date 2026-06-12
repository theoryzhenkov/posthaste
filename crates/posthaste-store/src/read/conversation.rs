use super::*;

impl ConversationReadStore for DatabaseStore {
    /// Returns a seek-paginated page of conversations, optionally filtered by
    /// account and/or mailbox.
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
    ) -> Result<ConversationPage, StoreError> {
        let connection = self.read_connection()?;
        query_conversations(
            &connection,
            "WHERE (?1 IS NULL OR m.account_id = ?1)
               AND (
                 ?2 IS NULL OR EXISTS (
                     SELECT 1
                     FROM message_mailbox mm
                     WHERE mm.account_id = m.account_id
                       AND mm.message_id = m.id
                       AND mm.mailbox_id = ?2
                 )
               )",
            vec![
                account_id
                    .map(|source| SqlValue::Text(source.as_str().to_string()))
                    .unwrap_or(SqlValue::Null),
                mailbox_id
                    .map(|mailbox| SqlValue::Text(mailbox.as_str().to_string()))
                    .unwrap_or(SqlValue::Null),
            ],
            limit,
            cursor,
            sort_field,
            sort_direction,
        )
    }

    /// Returns all messages in a conversation ordered by `received_at ASC`,
    /// or `None` if the conversation does not exist.
    ///
    /// @spec docs/L1-search#conversation-view
    fn get_conversation(
        &self,
        conversation_id: &ConversationId,
    ) -> Result<Option<ConversationView>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT m.id, m.account_id, a.name, m.thread_id, m.conversation_id, m.subject,
                        m.from_name, m.from_email, m.to_json, m.preview, m.received_at, m.has_attachment,
                        m.is_read, m.is_flagged
                 FROM conversation_message cm
                 JOIN message m
                   ON m.account_id = cm.account_id
                  AND m.id = cm.message_id
                 JOIN source_projection a
                   ON a.source_id = m.account_id
                 WHERE cm.conversation_id = ?1
                 ORDER BY m.received_at ASC, m.id ASC",
            )
            .map_err(sql_to_store_error)?;
        let rows = load_message_summary_rows(&mut statement, params![conversation_id.as_str()])?;
        let messages = hydrate_message_summaries(&connection, rows)?;
        if messages.is_empty() {
            return Ok(None);
        }
        let subject = messages
            .last()
            .and_then(|message| message.subject.clone())
            .or_else(|| messages.iter().find_map(|message| message.subject.clone()));
        Ok(Some(ConversationView {
            id: conversation_id.clone(),
            subject,
            messages,
        }))
    }
}
