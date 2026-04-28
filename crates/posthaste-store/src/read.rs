use super::*;

impl DatabaseStore {
    /// Lists all messages in a thread, ordered by `received_at ASC`.
    ///
    /// @spec docs/L1-search#thread-view
    fn list_messages_for_thread(
        &self,
        account_id: &AccountId,
        thread_id: &ThreadId,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT m.id, m.account_id, a.name, m.thread_id, m.conversation_id, m.subject,
                        m.from_name, m.from_email, m.preview, m.received_at, m.has_attachment,
                        m.is_read, m.is_flagged
                 FROM message m
                 JOIN source_projection a ON a.source_id = m.account_id
                 WHERE m.account_id = ?1 AND m.thread_id = ?2
                 ORDER BY received_at ASC",
            )
            .map_err(sql_to_store_error)?;
        let rows = load_message_summary_rows(
            &mut statement,
            params![account_id.as_str(), thread_id.as_str()],
        )?;
        hydrate_message_summaries(&connection, rows)
    }
}

impl MailboxReadStore for DatabaseStore {
    /// Lists mailboxes for an account, ordered by role then name.
    fn list_mailboxes(&self, account_id: &AccountId) -> Result<Vec<MailboxSummary>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, name, role, unread_emails, total_emails
                 FROM mailbox
                 WHERE account_id = ?1
                 ORDER BY COALESCE(role, ''), name",
            )
            .map_err(sql_to_store_error)?;

        let rows = statement
            .query_map(params![account_id.as_str()], |row| {
                Ok(MailboxSummary {
                    id: MailboxId(row.get(0)?),
                    name: row.get(1)?,
                    role: row.get(2)?,
                    unread_emails: row.get(3)?,
                    total_emails: row.get(4)?,
                })
            })
            .map_err(sql_to_store_error)?;
        let mailboxes = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)?;
        Ok(mailboxes)
    }
}

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
                        m.from_name, m.from_email, m.preview, m.received_at, m.has_attachment,
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
            "SELECT m.id, m.account_id, a.name, m.thread_id, m.conversation_id, m.subject,
                    m.from_name, m.from_email, m.preview, m.received_at, m.has_attachment,
                    m.is_read, m.is_flagged
             FROM message m
             JOIN source_projection a
               ON a.source_id = m.account_id
             JOIN message_mailbox mm
               ON mm.account_id = m.account_id
              AND mm.message_id = m.id
             WHERE m.account_id = ?1 AND mm.mailbox_id = ?2
             ORDER BY m.received_at DESC"
        } else {
            "SELECT m.id, m.account_id, a.name, m.thread_id, m.conversation_id, m.subject,
                    m.from_name, m.from_email, m.preview, m.received_at, m.has_attachment,
                    m.is_read, m.is_flagged
             FROM message m
             JOIN source_projection a
               ON a.source_id = m.account_id
             WHERE m.account_id = ?1
             ORDER BY m.received_at DESC"
        };
        let mut statement = connection.prepare(sql).map_err(sql_to_store_error)?;
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
                     FROM message_mailbox mm
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

impl TagReadStore for DatabaseStore {
    fn list_tags(&self, account_id: &AccountId) -> Result<Vec<TagSummary>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT TRIM(mk.keyword) AS keyword,
                        COUNT(DISTINCT CASE WHEN m.is_read = 0 THEN m.id END) AS unread_messages,
                        COUNT(DISTINCT m.id) AS total_messages
                 FROM message_keyword mk
                 JOIN message m
                   ON m.account_id = mk.account_id
                  AND m.id = mk.message_id
                 WHERE mk.account_id = ?1
                   AND TRIM(mk.keyword) <> ''
                   AND TRIM(mk.keyword) NOT LIKE '$%'
                 GROUP BY TRIM(mk.keyword)
                 ORDER BY LOWER(TRIM(mk.keyword)), TRIM(mk.keyword)",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params![account_id.as_str()], |row| {
                Ok(TagSummary {
                    name: row.get(0)?,
                    unread_messages: row.get(1)?,
                    total_messages: row.get(2)?,
                })
            })
            .map_err(sql_to_store_error)?;

        let mut tags = Vec::new();
        for row in rows {
            tags.push(row.map_err(sql_to_store_error)?);
        }
        Ok(tags)
    }
}

impl SmartMailboxStore for DatabaseStore {
    /// Evaluates a smart mailbox rule against all sources and returns matching
    /// messages.
    ///
    /// @spec docs/L1-search#smart-mailbox-data-model
    fn query_messages_by_rule(
        &self,
        rule: &SmartMailboxRule,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        let connection = self.read_connection()?;
        query_messages_by_rule(&connection, rule)
    }

    /// Evaluates a smart mailbox rule and returns a paginated message view.
    ///
    /// @spec docs/L1-api#cursor-pagination
    fn query_message_page_by_rule(
        &self,
        rule: &SmartMailboxRule,
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
        rule: &SmartMailboxRule,
        limit: usize,
        cursor: Option<&ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    ) -> Result<ConversationPage, StoreError> {
        let connection = self.read_connection()?;
        query_conversations_by_rule(&connection, rule, limit, cursor, sort_field, sort_direction)
    }

    /// Returns (unread, total) message counts for a smart mailbox rule.
    fn query_smart_mailbox_counts(
        &self,
        rule: &SmartMailboxRule,
    ) -> Result<(i64, i64), StoreError> {
        let connection = self.read_connection()?;
        count_smart_mailbox_messages(&connection, rule)
    }
}

impl MessageDetailStore for DatabaseStore {
    /// Returns full message detail including body (if fetched) and raw message
    /// reference.
    ///
    /// @spec docs/L1-sync#email-bodies-are-fetched-lazily
    fn get_message_detail(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT m.id, m.account_id, a.name, m.thread_id, m.conversation_id, m.subject,
                        m.from_name, m.from_email, m.preview, m.received_at, m.has_attachment,
                        m.is_read, m.is_flagged
                 FROM message m
                 JOIN source_projection a
                   ON a.source_id = m.account_id
                 WHERE m.account_id = ?1 AND m.id = ?2",
            )
            .map_err(sql_to_store_error)?;
        let rows = load_message_summary_rows(
            &mut statement,
            params![account_id.as_str(), message_id.as_str()],
        )?;
        let mut summaries = hydrate_message_summaries(&connection, rows)?;
        let Some(summary) = summaries.pop() else {
            return Ok(None);
        };

        let body = connection
            .query_row(
                "SELECT body_html, body_text, raw_path, raw_sha256, raw_size, raw_mime_type, fetched_at
                 FROM message_body
                 WHERE account_id = ?1 AND message_id = ?2",
                params![account_id.as_str(), message_id.as_str()],
                |row| {
                    let raw_path: Option<String> = row.get(2)?;
                    let raw_sha256: Option<String> = row.get(3)?;
                    let raw_size: Option<i64> = row.get(4)?;
                    let raw_mime_type: Option<String> = row.get(5)?;
                    let fetched_at: Option<String> = row.get(6)?;
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        raw_path.and_then(|path| {
                            Some(RawMessageRef {
                                path,
                                sha256: raw_sha256?,
                                size: raw_size?,
                                mime_type: raw_mime_type?,
                                fetched_at: fetched_at?,
                            })
                        }),
                    ))
                },
            )
            .optional()
            .map_err(sql_to_store_error)?;
        let attachments = fetch_message_attachments(&connection, account_id, message_id)?;

        Ok(Some(MessageDetail {
            summary,
            body_html: body.as_ref().and_then(|row| row.0.clone()),
            body_text: body.as_ref().and_then(|row| row.1.clone()),
            raw_message: body.and_then(|row| row.2),
            attachments,
        }))
    }

    /// Returns a thread view with all messages ordered chronologically, or
    /// `None` if empty.
    ///
    /// @spec docs/L1-search#thread-view
    fn get_thread(
        &self,
        account_id: &AccountId,
        thread_id: &ThreadId,
    ) -> Result<Option<ThreadView>, StoreError> {
        let messages = self.list_messages_for_thread(account_id, thread_id)?;
        if messages.is_empty() {
            return Ok(None);
        }
        Ok(Some(ThreadView {
            id: thread_id.clone(),
            messages,
        }))
    }
}

impl MessageMailboxStore for DatabaseStore {
    /// Returns the mailbox IDs a message belongs to.
    fn get_message_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Vec<MailboxId>, StoreError> {
        let connection = self.read_connection()?;
        fetch_mailbox_ids(&connection, account_id, message_id)
    }
}
