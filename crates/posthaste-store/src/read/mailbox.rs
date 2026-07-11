use super::*;

impl MailboxReadStore for DatabaseStore {
    /// Lists mailboxes for an account, ordered by role then name.
    ///
    /// Counts are a LIVE derivation over the `_effective` views (NS1): the
    /// same plane every strangled read serves, so a folded move/read/destroy
    /// shifts counts in the same instant it shifts the lists — one derivation,
    /// nothing materialized to drift (retires the DP-H12 counter-drift class
    /// along with the maintenance triggers). The GROUP BY rides
    /// `idx_message_mailbox (account_id, mailbox_id)`; the scan is bounded by
    /// the account's membership rows and this read is invalidation-driven,
    /// not hot.
    fn list_mailboxes(&self, account_id: &AccountId) -> Result<Vec<MailboxSummary>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare_cached(
                "SELECT b.id, b.name, b.role,
                        COALESCE(c.unread, 0), COALESCE(c.total, 0)
                 FROM mailbox b
                 LEFT JOIN (
                     SELECT mm.mailbox_id,
                            COUNT(*) AS total,
                            SUM(CASE WHEN m.is_read = 0 THEN 1 ELSE 0 END) AS unread
                     FROM message_mailbox_effective mm
                     JOIN message_effective m
                       ON m.account_id = mm.account_id
                      AND m.id = mm.message_id
                     WHERE mm.account_id = ?1
                     GROUP BY mm.mailbox_id
                 ) c ON c.mailbox_id = b.id
                 WHERE b.account_id = ?1
                 ORDER BY COALESCE(b.role, ''), b.name",
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
