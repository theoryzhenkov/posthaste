use super::*;

impl ImapSyncStateStore for DatabaseStore {
    fn list_imap_mailbox_states(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<ImapMailboxSyncState>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT mailbox_id, mailbox_name, uid_validity, highest_uid,
                        highest_modseq, updated_at
                 FROM imap_mailbox_sync_state
                 WHERE account_id = ?1
                 ORDER BY mailbox_name, mailbox_id",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params![account_id.as_str()], imap_mailbox_state_from_row)
            .map_err(sql_to_store_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)
    }

    fn get_imap_mailbox_state(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
    ) -> Result<Option<ImapMailboxSyncState>, StoreError> {
        let connection = self.read_connection()?;
        connection
            .query_row(
                "SELECT mailbox_id, mailbox_name, uid_validity, highest_uid,
                        highest_modseq, updated_at
                 FROM imap_mailbox_sync_state
                 WHERE account_id = ?1 AND mailbox_id = ?2",
                params![account_id.as_str(), mailbox_id.as_str()],
                imap_mailbox_state_from_row,
            )
            .optional()
            .map_err(sql_to_store_error)
    }
}

impl ImapSyncStateWriteStore for DatabaseStore {
    fn put_imap_mailbox_state(
        &self,
        account_id: &AccountId,
        state: &ImapMailboxSyncState,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "INSERT INTO imap_mailbox_sync_state (
                    account_id, mailbox_id, mailbox_name, uid_validity,
                    highest_uid, highest_modseq, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(account_id, mailbox_id) DO UPDATE SET
                    mailbox_name = excluded.mailbox_name,
                    uid_validity = excluded.uid_validity,
                    highest_uid = excluded.highest_uid,
                    highest_modseq = excluded.highest_modseq,
                    updated_at = excluded.updated_at",
                params![
                    account_id.as_str(),
                    state.mailbox_id.as_str(),
                    state.mailbox_name,
                    state.uid_validity.0,
                    state.highest_uid.map(|uid| uid.0),
                    state.highest_modseq.map(|modseq| modseq.0.to_string()),
                    state.updated_at,
                ],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    fn delete_imap_mailbox_state(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "DELETE FROM imap_mailbox_sync_state
                 WHERE account_id = ?1 AND mailbox_id = ?2",
                params![account_id.as_str(), mailbox_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }
}

impl ImapMessageLocationStore for DatabaseStore {
    fn list_imap_message_locations(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Vec<ImapMessageLocation>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT message_id, mailbox_id, uid_validity, uid, modseq, updated_at
                 FROM imap_message_location
                 WHERE account_id = ?1 AND message_id = ?2
                 ORDER BY mailbox_id",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(
                params![account_id.as_str(), message_id.as_str()],
                imap_message_location_from_row,
            )
            .map_err(sql_to_store_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)
    }

    fn list_imap_mailbox_message_locations(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
    ) -> Result<Vec<ImapMessageLocation>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT message_id, mailbox_id, uid_validity, uid, modseq, updated_at
                 FROM imap_message_location
                 WHERE account_id = ?1 AND mailbox_id = ?2
                 ORDER BY uid",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(
                params![account_id.as_str(), mailbox_id.as_str()],
                imap_message_location_from_row,
            )
            .map_err(sql_to_store_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)
    }
}

impl ImapMessageLocationWriteStore for DatabaseStore {
    fn put_imap_message_location(
        &self,
        account_id: &AccountId,
        location: &ImapMessageLocation,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "INSERT INTO imap_message_location (
                    account_id, message_id, mailbox_id, uid_validity, uid, modseq, updated_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(account_id, message_id, mailbox_id) DO UPDATE SET
                    uid_validity = excluded.uid_validity,
                    uid = excluded.uid,
                    modseq = excluded.modseq,
                    updated_at = excluded.updated_at",
                params![
                    account_id.as_str(),
                    location.message_id.as_str(),
                    location.mailbox_id.as_str(),
                    location.uid_validity.0,
                    location.uid.0,
                    location.modseq.map(|modseq| modseq.0.to_string()),
                    location.updated_at,
                ],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    fn delete_imap_message_locations(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| {
            tx.execute(
                "DELETE FROM imap_message_location
                 WHERE account_id = ?1 AND message_id = ?2",
                params![account_id.as_str(), message_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }
}

fn imap_mailbox_state_from_row(row: &Row<'_>) -> rusqlite::Result<ImapMailboxSyncState> {
    let uid_validity = u32_from_row(row, 2, "uid_validity")?;
    let highest_uid = optional_u32_from_row(row, 3, "highest_uid")?.map(ImapUid);
    let highest_modseq = optional_u64_text_from_row(row, 4, "highest_modseq")?.map(ImapModSeq);
    Ok(ImapMailboxSyncState {
        mailbox_id: MailboxId(row.get(0)?),
        mailbox_name: row.get(1)?,
        uid_validity: ImapUidValidity(uid_validity),
        highest_uid,
        highest_modseq,
        updated_at: row.get(5)?,
    })
}

fn imap_message_location_from_row(row: &Row<'_>) -> rusqlite::Result<ImapMessageLocation> {
    Ok(ImapMessageLocation {
        message_id: MessageId(row.get(0)?),
        mailbox_id: MailboxId(row.get(1)?),
        uid_validity: ImapUidValidity(u32_from_row(row, 2, "uid_validity")?),
        uid: ImapUid(u32_from_row(row, 3, "uid")?),
        modseq: optional_u64_text_from_row(row, 4, "modseq")?.map(ImapModSeq),
        updated_at: row.get(5)?,
    })
}

fn optional_u32_from_row(
    row: &Row<'_>,
    index: usize,
    name: &'static str,
) -> rusqlite::Result<Option<u32>> {
    let Some(value) = row.get::<_, Option<i64>>(index)? else {
        return Ok(None);
    };
    u32::try_from(value).map(Some).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{name} out of range: {err}"),
            )),
        )
    })
}

fn optional_u64_text_from_row(
    row: &Row<'_>,
    index: usize,
    name: &'static str,
) -> rusqlite::Result<Option<u64>> {
    let Some(value) = row.get::<_, Option<String>>(index)? else {
        return Ok(None);
    };
    value.parse::<u64>().map(Some).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{name} out of range: {err}"),
            )),
        )
    })
}

fn u32_from_row(row: &Row<'_>, index: usize, name: &'static str) -> rusqlite::Result<u32> {
    let value = row.get::<_, i64>(index)?;
    u32::try_from(value).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{name} out of range: {err}"),
            )),
        )
    })
}
