use super::*;

impl SyncStateStore for DatabaseStore {
    /// Returns all stored sync state tokens for an account.
    ///
    /// @spec docs/L1-sync#state-management
    fn get_sync_cursors(&self, account_id: &AccountId) -> Result<Vec<SyncCursor>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare_cached(
                "SELECT object_type, state, updated_at
                 FROM sync_cursor
                 WHERE account_id = ?1",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params![account_id.as_str()], |row| {
                Ok(SyncCursor {
                    object_type: parse_sync_object(&row.get::<_, String>(0)?)?,
                    state: row.get(1)?,
                    updated_at: row.get(2)?,
                })
            })
            .map_err(sql_to_store_error)?;
        let cursors = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)?;
        Ok(cursors)
    }

    /// Returns the sync state token for a specific object type, or `None` if
    /// no sync has occurred yet.
    ///
    /// @spec docs/L1-sync#state-management
    fn get_cursor(
        &self,
        account_id: &AccountId,
        object_type: SyncObject,
    ) -> Result<Option<SyncCursor>, StoreError> {
        let connection = self.read_connection()?;
        connection
            .query_row(
                "SELECT state, updated_at
                 FROM sync_cursor
                 WHERE account_id = ?1 AND object_type = ?2",
                params![account_id.as_str(), object_type.as_str()],
                |row| {
                    Ok(SyncCursor {
                        object_type,
                        state: row.get(0)?,
                        updated_at: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(sql_to_store_error)
    }
}
