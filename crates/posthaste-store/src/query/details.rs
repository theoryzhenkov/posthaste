use super::summaries::parse_recipients_json;
use super::*;

/// Fetches a single message's full detail (summary + body + raw ref) within
/// a transaction.
pub(crate) fn query_message_detail_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<MessageDetail>, StoreError> {
    let mut statement = tx
        .prepare_cached(
            "SELECT m.id, m.account_id, COALESCE(a.name, m.account_id), m.thread_id, m.conversation_id, m.subject,
                    m.from_name, m.from_email, m.to_json, m.preview, m.received_at, m.has_attachment,
                    m.is_read, m.is_flagged, m.draft_id, m.rfc_message_id, m.in_reply_to
             FROM message m
             LEFT JOIN source_projection a
               ON a.source_id = m.account_id
             WHERE m.account_id = ?1 AND m.id = ?2",
        )
        .map_err(sql_to_store_error)?;

    let detail = statement
        .query_row(params![account_id.as_str(), message_id.as_str()], |row| {
            Ok((
                (),
                MessageSummary {
                    id: MessageId(row.get(0)?),
                    source_id: AccountId(row.get(1)?),
                    source_name: row.get(2)?,
                    source_thread_id: ThreadId(row.get(3)?),
                    conversation_id: ConversationId(row.get(4)?),
                    subject: row.get(5)?,
                    from_name: row.get(6)?,
                    from_email: row.get(7)?,
                    to: parse_recipients_json(row.get(8)?)?,
                    preview: row.get(9)?,
                    received_at: row.get(10)?,
                    has_attachment: row.get::<_, i64>(11)? != 0,
                    is_read: row.get::<_, i64>(12)? != 0,
                    is_flagged: row.get::<_, i64>(13)? != 0,
                    mailbox_ids: Vec::new(),
                    keywords: Vec::new(),
                    version: None,
                    rfc_message_id: row.get(15)?,
                    in_reply_to: row.get(16)?,
                    draft_id: row.get(14)?,
                },
            ))
        })
        .optional()
        .map_err(sql_to_store_error)?;

    let Some(((), mut summary)) = detail else {
        return Ok(None);
    };

    summary.mailbox_ids = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
    summary.keywords = fetch_keywords_tx(tx, account_id, message_id)?;
    summary.version = fetch_message_version_tx(tx, account_id, message_id)?;

    let body = tx
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
    let attachments = fetch_message_attachments_tx(tx, account_id, message_id)?;

    Ok(Some(MessageDetail {
        summary,
        body_html: body.as_ref().and_then(|tuple| tuple.0.clone()),
        body_text: body.as_ref().and_then(|tuple| tuple.1.clone()),
        raw_message: body.and_then(|tuple| tuple.2),
        attachments,
    }))
}

/// Fetches a single message's summary (no body) within a transaction — the
/// canonical projection used both to serve list rows and, attached to a
/// `message.updated` event, to promote a never-held message at the store
/// (`firehose-carries-rows`). The SELECT mirrors the summary portion of
/// [`query_message_detail_tx`] so an event-promoted row is byte-identical to a
/// served one (one derivation — no second projection path).
pub(crate) fn query_message_summary_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<MessageSummary>, StoreError> {
    let mut statement = tx
        .prepare_cached(
            "SELECT m.id, m.account_id, COALESCE(a.name, m.account_id), m.thread_id, m.conversation_id, m.subject,
                    m.from_name, m.from_email, m.to_json, m.preview, m.received_at, m.has_attachment,
                    m.is_read, m.is_flagged, m.rfc_message_id, m.in_reply_to, m.draft_id
             FROM message m
             LEFT JOIN source_projection a
               ON a.source_id = m.account_id
             WHERE m.account_id = ?1 AND m.id = ?2",
        )
        .map_err(sql_to_store_error)?;
    let summary = statement
        .query_row(params![account_id.as_str(), message_id.as_str()], |row| {
            Ok(MessageSummary {
                id: MessageId(row.get(0)?),
                source_id: AccountId(row.get(1)?),
                source_name: row.get(2)?,
                source_thread_id: ThreadId(row.get(3)?),
                conversation_id: ConversationId(row.get(4)?),
                subject: row.get(5)?,
                from_name: row.get(6)?,
                from_email: row.get(7)?,
                to: parse_recipients_json(row.get(8)?)?,
                preview: row.get(9)?,
                received_at: row.get(10)?,
                has_attachment: row.get::<_, i64>(11)? != 0,
                is_read: row.get::<_, i64>(12)? != 0,
                is_flagged: row.get::<_, i64>(13)? != 0,
                mailbox_ids: Vec::new(),
                keywords: Vec::new(),
                version: None,
                rfc_message_id: row.get(14)?,
                in_reply_to: row.get(15)?,
                draft_id: row.get(16)?,
            })
        })
        .optional()
        .map_err(sql_to_store_error)?;
    let Some(mut summary) = summary else {
        return Ok(None);
    };
    summary.mailbox_ids = fetch_mailbox_ids_tx(tx, account_id, message_id)?;
    summary.keywords = fetch_keywords_tx(tx, account_id, message_id)?;
    summary.version = fetch_message_version_tx(tx, account_id, message_id)?;
    Ok(Some(summary))
}
