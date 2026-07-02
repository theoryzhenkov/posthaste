use super::*;
use crate::sql_cache::CachedSql;

pub(crate) fn upsert_body_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    body_html: Option<&str>,
    body_text: Option<&str>,
    raw_ref: Option<&RawMessageRef>,
) -> Result<(), StoreError> {
    tx.execute_cached(
        "INSERT INTO message_body (
            account_id, message_id, body_html, body_text, raw_path, raw_sha256, raw_size, raw_mime_type, fetched_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(account_id, message_id) DO UPDATE SET
            body_html = excluded.body_html,
            body_text = excluded.body_text,
            raw_path = excluded.raw_path,
            raw_sha256 = excluded.raw_sha256,
            raw_size = excluded.raw_size,
            raw_mime_type = excluded.raw_mime_type,
            fetched_at = excluded.fetched_at",
        params![
            account_id.as_str(),
            message_id.as_str(),
            body_html,
            body_text,
            raw_ref.map(|raw| raw.path.as_str()),
            raw_ref.map(|raw| raw.sha256.as_str()),
            raw_ref.map(|raw| raw.size),
            raw_ref.map(|raw| raw.mime_type.as_str()),
            raw_ref.map(|raw| raw.fetched_at.as_str()),
        ],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

/// Replaces the attachment metadata cached for a message with a fresh snapshot.
pub(crate) fn replace_attachments_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    attachments: &[MessageAttachment],
) -> Result<(), StoreError> {
    tx.execute_cached(
        "DELETE FROM message_attachment WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;

    for attachment in attachments {
        tx.execute_cached(
            "INSERT INTO message_attachment (
                account_id, message_id, id, blob_id, part_id, filename, mime_type, size,
                disposition, cid, is_inline
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                account_id.as_str(),
                message_id.as_str(),
                attachment.id.as_str(),
                attachment.blob_id.as_str(),
                attachment.part_id.as_deref(),
                attachment.filename.as_deref(),
                attachment.mime_type.as_str(),
                attachment.size,
                attachment.disposition.as_deref(),
                attachment.cid.as_deref(),
                bool_to_i64(attachment.is_inline),
            ],
        )
        .map_err(sql_to_store_error)?;
    }

    Ok(())
}

pub(crate) fn synthesize_raw_mime(message: &posthaste_domain_service::MessageRecord) -> Option<String> {
    if message.body_html.is_none() && message.body_text.is_none() {
        return None;
    }
    let subject = message.subject.as_deref().unwrap_or("(no subject)");
    let from = match (&message.from_name, &message.from_email) {
        (Some(name), Some(email)) => format!("{name} <{email}>"),
        (None, Some(email)) => email.clone(),
        _ => "unknown@example.invalid".to_string(),
    };
    let text = message
        .body_text
        .as_deref()
        .unwrap_or_else(|| message.preview.as_deref().unwrap_or(""));
    Some(synthesize_plain_text_raw_mime(&from, subject, Some(text)))
}
