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
