use super::*;

impl SenderAddressCacheStore for DatabaseStore {
    fn list_sender_address_cache(&self) -> Result<Vec<CachedSenderAddress>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT account_id, name, email, last_used_at
                 FROM sender_address_cache
                 ORDER BY last_used_at DESC, account_id ASC, normalized_email ASC",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(CachedSenderAddress {
                    source_id: AccountId::from(row.get::<_, String>(0)?),
                    name: row.get(1)?,
                    email: row.get(2)?,
                    last_used_at: row.get(3)?,
                })
            })
            .map_err(sql_to_store_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)
    }

    fn remember_sender_address(
        &self,
        account_id: &AccountId,
        sender: &Recipient,
    ) -> Result<(), StoreError> {
        let email = sender.email.trim();
        if !is_cacheable_sender_email(email) {
            return Ok(());
        }
        let normalized_email = email.to_ascii_lowercase();
        let name = sender
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty());
        let last_used_at = now_iso8601()?;
        self.write_transaction(|tx| {
            tx.execute(
                "INSERT INTO sender_address_cache (
                    account_id, normalized_email, email, name, last_used_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(account_id, normalized_email) DO UPDATE SET
                    email = excluded.email,
                    name = excluded.name,
                    last_used_at = excluded.last_used_at",
                params![
                    account_id.as_str(),
                    normalized_email,
                    email,
                    name,
                    last_used_at
                ],
            )
            .map_err(sql_to_store_error)?;
            tx.execute(
                "DELETE FROM sender_address_cache
                 WHERE account_id = ?1
                   AND normalized_email IN (
                     SELECT normalized_email
                     FROM sender_address_cache
                     WHERE account_id = ?1
                     ORDER BY last_used_at DESC, normalized_email ASC
                     LIMIT -1 OFFSET 40
                   )",
                params![account_id.as_str()],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }
}

fn is_cacheable_sender_email(email: &str) -> bool {
    if email.is_empty()
        || email.contains('*')
        || email.chars().any(|character| character.is_whitespace())
    {
        return false;
    }
    let mut parts = email.split('@');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(local), Some(domain), None) if !local.is_empty() && !domain.is_empty()
    )
}
