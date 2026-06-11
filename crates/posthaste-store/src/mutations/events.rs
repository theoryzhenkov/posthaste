use super::*;

/// Queries the `event_log` table with optional filters (account, seq cursor,
/// topic, mailbox). Returns events ordered by `seq ASC`.
///
/// @spec docs/L1-sync#event-propagation
pub(crate) fn list_events(
    connection: &Connection,
    filter: &EventFilter,
) -> Result<Vec<DomainEvent>, StoreError> {
    let mut sql = "SELECT seq, account_id, topic, occurred_at, mailbox_id, message_id, payload
         FROM event_log
         WHERE 1 = 1"
        .to_string();
    let mut bindings: Vec<SqlValue> = Vec::new();

    if let Some(account_id) = &filter.account_id {
        sql.push_str(" AND account_id = ?");
        sql.push_str(&(bindings.len() + 1).to_string());
        bindings.push(SqlValue::Text(account_id.to_string()));
    }

    if let Some(after_seq) = filter.after_seq {
        sql.push_str(" AND seq > ?");
        sql.push_str(&(bindings.len() + 1).to_string());
        bindings.push(SqlValue::Integer(after_seq));
    }
    if let Some(topic) = &filter.topic {
        sql.push_str(" AND topic = ?");
        sql.push_str(&(bindings.len() + 1).to_string());
        bindings.push(SqlValue::Text(topic.clone()));
    }
    if let Some(mailbox_id) = &filter.mailbox_id {
        sql.push_str(" AND mailbox_id = ?");
        sql.push_str(&(bindings.len() + 1).to_string());
        bindings.push(SqlValue::Text(mailbox_id.to_string()));
    }
    sql.push_str(" ORDER BY seq ASC");

    let mut statement = connection.prepare(&sql).map_err(sql_to_store_error)?;
    let params_ref = rusqlite::params_from_iter(bindings);
    let rows = statement
        .query_map(params_ref, row_to_event)
        .map_err(sql_to_store_error)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(sql_to_store_error)
}
