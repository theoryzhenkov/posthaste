use super::*;

impl MailboxReadStore for DatabaseStore {
    /// Lists mailboxes for an account, ordered by role then name.
    fn list_mailboxes(&self, account_id: &AccountId) -> Result<Vec<MailboxSummary>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, name, role, unread_emails, total_emails
                 FROM mailbox
                 WHERE account_id = ?1
                 ORDER BY COALESCE(role, ''), name",
            )
            .map_err(sql_to_store_error)?;

        let rows = statement
            .query_map(params![account_id.as_str()], |row| {
                Ok(MailboxSummary {
                    id: MailboxId(row.get(0)?),
                    name: row.get(1)?,
                    role: row.get(2)?,
                    unread_emails: row.get(3)?,
                    total_emails: row.get(4)?,
                })
            })
            .map_err(sql_to_store_error)?;
        let mailboxes = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)?;
        Ok(mailboxes)
    }
}

impl MessageMailboxStore for DatabaseStore {
    /// Returns the mailbox IDs a message belongs to.
    fn get_message_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Vec<MailboxId>, StoreError> {
        let connection = self.read_connection()?;
        fetch_mailbox_ids(&connection, account_id, message_id)
    }
}
