use super::*;

impl SourceProjectionStore for DatabaseStore {
    /// Creates or updates the `source_projection` row that maps account IDs to
    /// display names for query joins.
    fn upsert_source_projection(
        &self,
        source_id: &AccountId,
        name: &str,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "INSERT INTO source_projection (source_id, name) VALUES (?1, ?2)
                 ON CONFLICT(source_id) DO UPDATE SET name = excluded.name",
                params![source_id.as_str(), name],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    /// Removes a source projection row when an account is deleted.
    fn delete_source_projection(&self, source_id: &AccountId) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "DELETE FROM source_projection WHERE source_id = ?1",
                params![source_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }
}

impl SourceDataStore for DatabaseStore {
    /// Removes all data for an account from every table, including orphaned
    /// conversations.
    fn delete_source_data(&self, account_id: &AccountId) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "DELETE FROM mailbox WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM mailbox_role_override WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM message WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM message_mailbox WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM message_keyword WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM message_body WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM message_attachment WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM conversation_message WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM thread_view WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM sync_cursor WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM imap_mailbox_sync_state WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM imap_message_location WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM event_log WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM automation_backfill_job WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM address_book WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM cache_rescore_queue WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM cache_message_signal WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM cache_object WHERE account_id = ?1",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            cleanup_orphan_conversations_tx(tx)?;
            Ok(())
        })
    }
}
