use super::*;

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
