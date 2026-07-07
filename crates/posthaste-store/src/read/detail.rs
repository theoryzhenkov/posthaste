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
            .prepare_cached(
                "SELECT m.id, m.account_id, COALESCE(a.name, m.account_id), m.thread_id, m.conversation_id, m.subject,
                        m.from_name, m.from_email, m.to_json, m.preview, m.received_at, m.has_attachment,
                        m.is_read, m.is_flagged
                 FROM message m
                 LEFT JOIN source_projection a ON a.source_id = m.account_id
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

/// Read one message's summary (header-level projection) by id, without touching
/// the body or attachments. Shared by `get_message_detail` (which then adds the
/// body/attachments) and `get_message_summary` (which stops here).
fn read_message_summary(
    connection: &Connection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<MessageSummary>, StoreError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT m.id, m.account_id, COALESCE(a.name, m.account_id), m.thread_id, m.conversation_id, m.subject,
                    m.from_name, m.from_email, m.to_json, m.preview, m.received_at, m.has_attachment,
                    m.is_read, m.is_flagged
             FROM message m
             LEFT JOIN source_projection a
               ON a.source_id = m.account_id
             WHERE m.account_id = ?1 AND m.id = ?2",
        )
        .map_err(sql_to_store_error)?;
    let rows = load_message_summary_rows(
        &mut statement,
        params![account_id.as_str(), message_id.as_str()],
    )?;
    Ok(hydrate_message_summaries(connection, rows)?.pop())
}

/// Reads the parsed `List-Unsubscribe` targets stored on the message row, if
/// any. Detail-only projection — summaries never carry it.
fn read_list_unsubscribe(
    connection: &Connection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<ListUnsubscribe>, StoreError> {
    let json = connection
        .query_row(
            "SELECT list_unsubscribe FROM message WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), message_id.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(sql_to_store_error)?
        .flatten();
    // A row that fails to deserialize (schema drift) degrades to "no target"
    // rather than failing the whole detail read.
    Ok(json.and_then(|json| serde_json::from_str(&json).ok()))
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
        let Some(summary) = read_message_summary(&connection, account_id, message_id)? else {
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
        let list_unsubscribe = read_list_unsubscribe(&connection, account_id, message_id)?;

        Ok(Some(MessageDetail {
            summary,
            body_html: body.as_ref().and_then(|row| row.0.clone()),
            body_text: body.as_ref().and_then(|row| row.1.clone()),
            raw_message: body.and_then(|row| row.2),
            attachments,
            list_unsubscribe,
        }))
    }

    /// Cheap summary read: skips the body and attachment queries entirely so
    /// metadata-only callers (mailbox membership, keywords, existence) never
    /// materialize the body.
    fn get_message_summary(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageSummary>, StoreError> {
        let connection = self.read_connection()?;
        read_message_summary(&connection, account_id, message_id)
    }

    /// Detail read without the body: header + attachments, skipping the
    /// `message_body` query so the body is never materialized for the detail
    /// surface (the body is the separate `/body` resource).
    fn get_message_detail_without_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageDetail>, StoreError> {
        let connection = self.read_connection()?;
        let Some(summary) = read_message_summary(&connection, account_id, message_id)? else {
            return Ok(None);
        };
        let attachments = fetch_message_attachments(&connection, account_id, message_id)?;
        let list_unsubscribe = read_list_unsubscribe(&connection, account_id, message_id)?;
        Ok(Some(MessageDetail {
            summary,
            body_html: None,
            body_text: None,
            raw_message: None,
            attachments,
            list_unsubscribe,
        }))
    }

    /// Reads the cached raw RFC822 bytes for a message from its content-
    /// addressed file, or `None` when no raw body has been cached.
    ///
    /// @spec docs/L1-sync#email-bodies-are-fetched-lazily
    fn read_raw_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        let connection = self.read_connection()?;
        let raw_path = connection
            .query_row(
                "SELECT raw_path FROM message_body
                 WHERE account_id = ?1 AND message_id = ?2",
                params![account_id.as_str(), message_id.as_str()],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(sql_to_store_error)?
            .flatten();
        let Some(raw_path) = raw_path else {
            return Ok(None);
        };
        match std::fs::read(&raw_path) {
            Ok(bytes) => Ok(Some(bytes)),
            // A missing file means the cache was pruned; treat as not cached.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(io_to_store_error(error)),
        }
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
