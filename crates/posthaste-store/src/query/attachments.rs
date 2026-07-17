use super::*;

impl DatabaseStore {
    /// Locate the message that owns an attachment blob: the owning account
    /// and message ids plus the attachment metadata (mime type, filename,
    /// size) for serving the blob over the API. Returns the first matching
    /// row when a blob is referenced by more than one message (identical
    /// content), which is correct because the blob bytes are the same.
    pub fn find_attachment_by_blob(
        &self,
        blob_id: &BlobId,
    ) -> Result<Option<(AccountId, MessageId, MessageAttachment)>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare_cached(
                "SELECT account_id, message_id,
                        id, blob_id, part_id, filename, mime_type, size, disposition, cid, is_inline
                 FROM message_attachment
                 WHERE blob_id = ?1
                 ORDER BY account_id ASC, message_id ASC, id ASC
                 LIMIT 1",
            )
            .map_err(sql_to_store_error)?;
        let mut rows = statement
            .query(params![blob_id.as_str()])
            .map_err(sql_to_store_error)?;
        let Some(row) = rows.next().map_err(sql_to_store_error)? else {
            return Ok(None);
        };
        let account_id: String = row.get(0).map_err(sql_to_store_error)?;
        let message_id: String = row.get(1).map_err(sql_to_store_error)?;
        let attachment = MessageAttachment {
            id: row.get(2).map_err(sql_to_store_error)?,
            blob_id: BlobId(row.get(3).map_err(sql_to_store_error)?),
            part_id: row.get(4).map_err(sql_to_store_error)?,
            filename: row.get(5).map_err(sql_to_store_error)?,
            mime_type: row.get(6).map_err(sql_to_store_error)?,
            size: row.get(7).map_err(sql_to_store_error)?,
            disposition: row.get(8).map_err(sql_to_store_error)?,
            cid: row.get(9).map_err(sql_to_store_error)?,
            is_inline: row.get::<_, i64>(10).map_err(sql_to_store_error)? != 0,
        };
        Ok(Some((
            AccountId::from(account_id.as_str()),
            MessageId::from(message_id.as_str()),
            attachment,
        )))
    }
}

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
        .prepare_cached(
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
        .prepare_cached(
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
