use super::*;

impl MessageListStore for DatabaseStore {
    /// Lists messages for an account, optionally filtered by mailbox, ordered
    /// by `received_at DESC`.
    fn list_messages(
        &self,
        account_id: &AccountId,
        mailbox_id: Option<&MailboxId>,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        let connection = self.read_connection()?;
        let sql = if mailbox_id.is_some() {
            "SELECT m.id, m.account_id, COALESCE(a.name, m.account_id), m.thread_id, m.conversation_id, m.subject,
                    m.from_name, m.from_email, m.to_json, m.preview, m.received_at, m.has_attachment,
                    m.is_read, m.is_flagged
             FROM message_effective m
             LEFT JOIN source_projection a
               ON a.source_id = m.account_id
             JOIN message_mailbox_effective mm
               ON mm.account_id = m.account_id
              AND mm.message_id = m.id
             WHERE m.account_id = ?1 AND mm.mailbox_id = ?2
             ORDER BY m.received_at DESC"
        } else {
            "SELECT m.id, m.account_id, COALESCE(a.name, m.account_id), m.thread_id, m.conversation_id, m.subject,
                    m.from_name, m.from_email, m.to_json, m.preview, m.received_at, m.has_attachment,
                    m.is_read, m.is_flagged
             FROM message_effective m
             LEFT JOIN source_projection a
               ON a.source_id = m.account_id
             WHERE m.account_id = ?1
             ORDER BY m.received_at DESC"
        };
        let mut statement = connection.prepare_cached(sql).map_err(sql_to_store_error)?;
        let summary_rows = if let Some(mailbox_id) = mailbox_id {
            load_message_summary_rows(
                &mut statement,
                params![account_id.as_str(), mailbox_id.as_str()],
            )?
        } else {
            load_message_summary_rows(&mut statement, params![account_id.as_str()])?
        };
        hydrate_message_summaries(&connection, summary_rows)
    }

    /// Returns a seek-paginated page of messages for an account, optionally
    /// filtered by mailbox.
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
    ) -> Result<MessagePage, StoreError> {
        let connection = self.read_connection()?;
        query_message_page(
            &connection,
            "WHERE m.account_id = ?1
               AND (
                 ?2 IS NULL OR EXISTS (
                     SELECT 1
                     FROM message_mailbox_effective mm
                     WHERE mm.account_id = m.account_id
                       AND mm.message_id = m.id
                       AND mm.mailbox_id = ?2
                 )
               )",
            vec![
                SqlValue::Text(account_id.as_str().to_string()),
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
}
