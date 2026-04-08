use super::*;
use posthaste_domain::MessageAttachment;

/// Determines the conversation ID for a message using a three-tier lookup:
/// 1. Match by RFC `Message-ID`, `In-Reply-To`, or `References` headers
/// 2. Match by server-assigned `thread_id`
/// 3. Match by normalized subject
///
/// If multiple conversations match, they are merged into the lowest-sorted ID.
/// If none match, a new deterministic ID is generated.
///
/// @spec docs/L1-sync#sqlite-schema
pub(crate) fn assign_conversation_id_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message: &posthaste_domain::MessageRecord,
) -> Result<ConversationId, StoreError> {
    let mut matches = HashSet::new();
    let mut header_ids = Vec::new();
    if let Some(message_id) = normalize_header_token(message.rfc_message_id.as_deref()) {
        header_ids.push(message_id);
    }
    if let Some(in_reply_to) = normalize_header_token(message.in_reply_to.as_deref()) {
        header_ids.push(in_reply_to);
    }
    for reference in &message.references {
        if let Some(reference) = normalize_header_token(Some(reference.as_str())) {
            header_ids.push(reference);
        }
    }
    for header_id in header_ids {
        let mut statement = tx
            .prepare(
                "SELECT DISTINCT conversation_id
                 FROM message
                 WHERE rfc_message_id = ?1 AND conversation_id IS NOT NULL",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params![header_id], |row| row.get::<_, String>(0))
            .map_err(sql_to_store_error)?;
        for conversation_id in rows {
            matches.insert(conversation_id.map_err(sql_to_store_error)?);
        }
    }

    if matches.is_empty() {
        let by_thread = tx
            .query_row(
                "SELECT conversation_id
                 FROM message
                 WHERE account_id = ?1 AND thread_id = ?2 AND conversation_id IS NOT NULL
                 ORDER BY received_at DESC
                 LIMIT 1",
                params![account_id.as_str(), message.source_thread_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(sql_to_store_error)?;
        if let Some(conversation_id) = by_thread {
            matches.insert(conversation_id);
        }
    }

    if matches.is_empty() {
        if let Some(normalized_subject_value) = normalized_subject(message.subject.as_deref()) {
            let by_subject = tx
                .query_row(
                    "SELECT conversation_id
                     FROM message
                     WHERE account_id = ?1
                       AND normalized_subject = ?2
                       AND conversation_id IS NOT NULL
                     ORDER BY received_at DESC
                     LIMIT 1",
                    params![account_id.as_str(), normalized_subject_value],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(sql_to_store_error)?;
            if let Some(conversation_id) = by_subject {
                matches.insert(conversation_id);
            }
        }
    }

    let mut matches = matches.into_iter().collect::<Vec<_>>();
    matches.sort();
    let target = matches
        .first()
        .map(|conversation_id| ConversationId::from(conversation_id.as_str()))
        .unwrap_or_else(|| generate_conversation_id(account_id, message));
    if matches.len() > 1 {
        merge_conversations_tx(tx, &target, &matches[1..])?;
    }
    Ok(target)
}

/// Recomputes the `conversation` projection row (subject, latest message,
/// counts) from the linked messages. Deletes the row if no messages remain.
///
/// @spec docs/L1-sync#sqlite-schema
pub(crate) fn refresh_conversation_projection_tx(
    tx: &Transaction<'_>,
    conversation_id: &ConversationId,
) -> Result<(), StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT m.account_id, m.id, m.subject, m.normalized_subject, m.received_at, m.is_read
             FROM conversation_message cm
             JOIN message m
               ON m.account_id = cm.account_id
              AND m.id = cm.message_id
             WHERE cm.conversation_id = ?1
             ORDER BY m.received_at DESC, m.id DESC",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![conversation_id.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;

    if rows.is_empty() {
        tx.execute(
            "DELETE FROM conversation WHERE id = ?1",
            params![conversation_id.as_str()],
        )
        .map_err(sql_to_store_error)?;
        return Ok(());
    }

    let latest = &rows[0];
    let subject = latest
        .2
        .clone()
        .or_else(|| rows.iter().find_map(|row| row.2.clone()));
    let normalized_subject_value = latest
        .3
        .clone()
        .or_else(|| rows.iter().find_map(|row| row.3.clone()));
    let unread_count = rows.iter().filter(|row| row.5 == 0).count() as i64;
    tx.execute(
        "INSERT INTO conversation (
            id, subject, normalized_subject, latest_received_at, latest_source_id,
            latest_message_id, message_count, unread_count
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
            subject = excluded.subject,
            normalized_subject = excluded.normalized_subject,
            latest_received_at = excluded.latest_received_at,
            latest_source_id = excluded.latest_source_id,
            latest_message_id = excluded.latest_message_id,
            message_count = excluded.message_count,
            unread_count = excluded.unread_count",
        params![
            conversation_id.as_str(),
            subject,
            normalized_subject_value,
            &latest.4,
            &latest.0,
            &latest.1,
            rows.len() as i64,
            unread_count,
        ],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

/// Removes conversation rows that have no linked messages.
pub(crate) fn cleanup_orphan_conversations_tx(tx: &Transaction<'_>) -> Result<(), StoreError> {
    tx.execute(
        "DELETE FROM conversation
         WHERE id NOT IN (SELECT DISTINCT conversation_id FROM conversation_message)",
        [],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

/// Recomputes the `thread_view` row with the ordered list of email IDs.
/// Deletes the row if no messages remain in the thread.
pub(crate) fn refresh_thread_projection_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    thread_id: &ThreadId,
) -> Result<(), StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT id
             FROM message
             WHERE account_id = ?1 AND thread_id = ?2
             ORDER BY received_at ASC",
        )
        .map_err(sql_to_store_error)?;
    let email_ids = statement
        .query_map(params![account_id.as_str(), thread_id.as_str()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(sql_to_store_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)?;
    if email_ids.is_empty() {
        tx.execute(
            "DELETE FROM thread_view WHERE account_id = ?1 AND id = ?2",
            params![account_id.as_str(), thread_id.as_str()],
        )
        .map_err(sql_to_store_error)?;
    } else {
        tx.execute(
            "INSERT INTO thread_view (account_id, id, email_ids)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(account_id, id) DO UPDATE SET email_ids = excluded.email_ids",
            params![
                account_id.as_str(),
                thread_id.as_str(),
                serde_json::to_string(&email_ids)
                    .map_err(|err| StoreError::Failure(err.to_string()))?
            ],
        )
        .map_err(sql_to_store_error)?;
    }
    Ok(())
}

/// Recomputes `total_emails` and `unread_emails` on the `mailbox` row from
/// the `message_mailbox` junction.
pub(crate) fn refresh_mailbox_counters_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    mailbox_id: &MailboxId,
) -> Result<(), StoreError> {
    let (total, unread) = tx
        .query_row(
            "SELECT COUNT(*), SUM(CASE WHEN m.is_read = 0 THEN 1 ELSE 0 END)
             FROM message_mailbox mm
             JOIN message m
               ON m.account_id = mm.account_id
              AND m.id = mm.message_id
             WHERE mm.account_id = ?1 AND mm.mailbox_id = ?2",
            params![account_id.as_str(), mailbox_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                ))
            },
        )
        .map_err(sql_to_store_error)?;
    tx.execute(
        "UPDATE mailbox
         SET total_emails = ?3,
             unread_emails = ?4
         WHERE account_id = ?1 AND id = ?2",
        params![account_id.as_str(), mailbox_id.as_str(), total, unread],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

/// Upserts HTML/text body and raw message reference into `message_body`.
pub(crate) fn upsert_body_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    body_html: Option<&str>,
    body_text: Option<&str>,
    raw_ref: Option<&RawMessageRef>,
) -> Result<(), StoreError> {
    tx.execute(
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
    tx.execute(
        "DELETE FROM message_attachment WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;

    for attachment in attachments {
        tx.execute(
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

/// Deletes a message and all its junction rows (keywords, mailboxes, body,
/// conversation link).
pub(crate) fn delete_message_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<(), StoreError> {
    tx.execute(
        "DELETE FROM message_keyword WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM message_mailbox WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM message_body WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM message_attachment WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM conversation_message WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    tx.execute(
        "DELETE FROM message WHERE account_id = ?1 AND id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

/// Inserts a domain event into `event_log` with a monotonically increasing
/// `seq`.
///
/// @spec docs/L1-sync#event-propagation
pub(crate) fn insert_event_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    topic: &str,
    mailbox_id: Option<&MailboxId>,
    message_id: Option<&MessageId>,
    payload: Value,
) -> Result<DomainEvent, StoreError> {
    let occurred_at = now_iso8601()?;
    tx.execute(
        "INSERT INTO event_log (account_id, topic, occurred_at, mailbox_id, message_id, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            account_id.as_str(),
            topic,
            occurred_at,
            mailbox_id.map(MailboxId::as_str),
            message_id.map(MessageId::as_str),
            payload.to_string()
        ],
    )
    .map_err(sql_to_store_error)?;
    let seq = tx.last_insert_rowid();
    Ok(DomainEvent {
        seq,
        account_id: account_id.clone(),
        topic: topic.to_string(),
        occurred_at,
        mailbox_id: mailbox_id.cloned(),
        message_id: message_id.cloned(),
        payload,
    })
}

/// Merges multiple conversations into a single target by reassigning all
/// messages and cleaning up old conversation rows.
fn merge_conversations_tx(
    tx: &Transaction<'_>,
    target: &ConversationId,
    other_ids: &[String],
) -> Result<(), StoreError> {
    for other_id in other_ids {
        tx.execute(
            "UPDATE message
             SET conversation_id = ?1
             WHERE conversation_id = ?2",
            params![target.as_str(), other_id],
        )
        .map_err(sql_to_store_error)?;
        tx.execute(
            "UPDATE OR REPLACE conversation_message
             SET conversation_id = ?1
             WHERE conversation_id = ?2",
            params![target.as_str(), other_id],
        )
        .map_err(sql_to_store_error)?;
        tx.execute("DELETE FROM conversation WHERE id = ?1", params![other_id])
            .map_err(sql_to_store_error)?;
    }
    Ok(())
}

/// Trims whitespace from header tokens, returning `None` for empty/absent
/// values.
fn normalize_header_token(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Strips `Re:`/`Fwd:` prefixes and lowercases the subject for conversation
/// grouping.
pub(crate) fn normalized_subject(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let mut normalized = value.trim();
        loop {
            let lower = normalized.to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("re:") {
                normalized = normalized[normalized.len() - rest.len()..].trim();
                continue;
            }
            if let Some(rest) = lower.strip_prefix("fwd:") {
                normalized = normalized[normalized.len() - rest.len()..].trim();
                continue;
            }
            break;
        }
        if normalized.is_empty() {
            None
        } else {
            Some(normalized.to_ascii_lowercase())
        }
    })
}

/// Generates a deterministic conversation ID from account ID, message ID,
/// RFC `Message-ID`, and subject via SHA-256.
fn generate_conversation_id(
    account_id: &AccountId,
    message: &posthaste_domain::MessageRecord,
) -> ConversationId {
    let mut hasher = Sha256::new();
    hasher.update(account_id.as_str().as_bytes());
    hasher.update(message.id.as_str().as_bytes());
    if let Some(rfc_message_id) = &message.rfc_message_id {
        hasher.update(rfc_message_id.as_bytes());
    }
    if let Some(subject) = &message.subject {
        hasher.update(subject.as_bytes());
    }
    ConversationId(format!("conv-{}", hex_encode(hasher.finalize())))
}

/// Synthesizes a minimal RFC 822 message from available body/metadata when no
/// raw MIME was provided by the sync layer.
pub(crate) fn synthesize_raw_mime(message: &posthaste_domain::MessageRecord) -> Option<String> {
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
        .unwrap_or(message.preview.as_deref().unwrap_or(""));
    Some(synthesize_plain_text_raw_mime(&from, subject, Some(text)))
}
