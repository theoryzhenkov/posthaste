use super::*;
use crate::sql_cache::CachedSql;

pub(crate) fn fetch_mailbox_ids(
    connection: &Connection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<MailboxId>, StoreError> {
    let mut statement = connection
        .prepare_cached(
            "SELECT mailbox_id
             FROM message_mailbox
             WHERE account_id = ?1 AND message_id = ?2
             ORDER BY mailbox_id",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            Ok(MailboxId(row.get(0)?))
        })
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}

/// Fetches mailbox IDs for a single message (within a transaction).
pub(crate) fn fetch_mailbox_ids_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<MailboxId>, StoreError> {
    let mut statement = tx
        .prepare_cached(
            "SELECT mailbox_id
             FROM message_mailbox
             WHERE account_id = ?1 AND message_id = ?2
             ORDER BY mailbox_id",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            Ok(MailboxId(row.get(0)?))
        })
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}

/// Fetches keywords for a single message (within a transaction).
pub(crate) fn fetch_keywords_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<String>, StoreError> {
    let mut statement = tx
        .prepare_cached(
            "SELECT keyword
             FROM message_keyword
             WHERE account_id = ?1 AND message_id = ?2
             ORDER BY keyword",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(params![account_id.as_str(), message_id.as_str()], |row| {
            row.get(0)
        })
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}

/// Reads a message's per-message authority-state version within a transaction:
/// the max IMAP `modseq` across its locations (stored as TEXT, so `CAST AS
/// INTEGER` for numeric ordering). `None` when the message has no IMAP location
/// with a modseq (JMAP / mock / local) — those providers have no per-message
/// version, and the client leaves such bases unguarded.
/// @spec docs/eph/DESIGN-L2-message-authority-version
pub(crate) fn fetch_message_version_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Option<u64>, StoreError> {
    let version: Option<i64> = tx
        .query_row(
            "SELECT MAX(CAST(modseq AS INTEGER))
             FROM imap_message_location
             WHERE account_id = ?1 AND message_id = ?2 AND modseq IS NOT NULL",
            params![account_id.as_str(), message_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_to_store_error)?
        .flatten();
    Ok(version.map(|value| value as u64))
}

/// Reads the current `unread_emails`/`total_emails` for a set of mailboxes
/// within a transaction — the authoritative count point-read attached to a
/// `message.updated` event so the reactive store's `mailbox[id].count` updates
/// in the same atomic batch as the row delta (`D3`, `counts-on-the-stream`).
/// Trigger-maintained, so the read is consistent at event-emit time. Mailboxes
/// absent from the local `mailbox` table are skipped (no row to read).
pub(crate) fn mailbox_counts_json_tx<'a>(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    mailbox_ids: impl Iterator<Item = &'a MailboxId>,
) -> Result<Value, StoreError> {
    let mut deltas = Vec::new();
    for mailbox_id in mailbox_ids {
        let counts = tx
            .query_row_cached(
                "SELECT unread_emails, total_emails
                 FROM mailbox
                 WHERE account_id = ?1 AND id = ?2",
                params![account_id.as_str(), mailbox_id.as_str()],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
            .map_err(sql_to_store_error)?;
        if let Some((unread, total)) = counts {
            deltas.push(json!({
                "mailboxId": mailbox_id.as_str(),
                "unreadCount": unread,
                "totalCount": total,
            }));
        }
    }
    Ok(Value::Array(deltas))
}
