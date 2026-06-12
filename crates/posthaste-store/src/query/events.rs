use super::*;

/// Maps an `event_log` row to a `DomainEvent`.
pub(crate) fn row_to_event(row: &rusqlite::Row<'_>) -> Result<DomainEvent, rusqlite::Error> {
    let payload: String = row.get(6)?;
    Ok(DomainEvent {
        seq: row.get(0)?,
        account_id: AccountId(row.get(1)?),
        topic: row.get(2)?,
        occurred_at: row.get(3)?,
        mailbox_id: row.get::<_, Option<String>>(4)?.map(MailboxId),
        message_id: row.get::<_, Option<String>>(5)?.map(MessageId),
        payload: serde_json::from_str(&payload).unwrap_or_else(|_| json!({})),
    })
}
