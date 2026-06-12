use super::*;

fn row_to_message_attachment(
    row: &rusqlite::Row<'_>,
) -> Result<MessageAttachment, rusqlite::Error> {
    Ok(MessageAttachment {
        id: row.get(0)?,
        blob_id: BlobId(row.get(1)?),
        part_id: row.get(2)?,
        filename: row.get(3)?,
        mime_type: row.get(4)?,
        size: row.get(5)?,
        disposition: row.get(6)?,
        cid: row.get(7)?,
        is_inline: row.get::<_, i64>(8)? != 0,
    })
}

pub(crate) fn fetch_message_attachments(
    connection: &Connection,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<MessageAttachment>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT id, blob_id, part_id, filename, mime_type, size, disposition, cid, is_inline
             FROM message_attachment
             WHERE account_id = ?1 AND message_id = ?2
             ORDER BY id ASC",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(
            params![account_id.as_str(), message_id.as_str()],
            row_to_message_attachment,
        )
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}

pub(crate) fn fetch_message_attachments_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<Vec<MessageAttachment>, StoreError> {
    let mut statement = tx
        .prepare(
            "SELECT id, blob_id, part_id, filename, mime_type, size, disposition, cid, is_inline
             FROM message_attachment
             WHERE account_id = ?1 AND message_id = ?2
             ORDER BY id ASC",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(
            params![account_id.as_str(), message_id.as_str()],
            row_to_message_attachment,
        )
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}
