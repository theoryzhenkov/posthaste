//! Snooze store operations: insert/delete a message's return-time row + list
//! due rows for the scheduler tick. The `message_snooze` table is
//! Posthaste-local (not provider-synced); see `DESIGN-L2-snooze`.
//!
//! @spec docs/eph/DESIGN-L2-snooze

use super::*;

use crate::sql_cache::CachedSql;

/// Bound on `list_due_snoozes` per scheduler tick. The snooze scheduler
/// (`handle_snooze_tick`) already re-invokes this every `SNOOZE_INTERVAL` and
/// each returned message's row is deleted (`clear_snooze_on_mailbox_replace_tx`)
/// as it is processed, so a bounded batch per tick drains a mass-snooze
/// backlog across ticks instead of materializing it all into one unbounded
/// `Vec` (N15 / RFC-L2-lifecycle D67(b) / M27 sub-unit (b)). Local-store-only
/// writes (no provider round trip), so this can be generous relative to
/// `ARM_BUDGET_SNOOZE`'s tight 30s budget. **Review**.
pub(crate) const SNOOZE_DUE_BATCH_LIMIT: i64 = 500;

impl DatabaseStore {
    /// Insert (or replace) a snooze row. Called by `message.snooze` after the
    /// move to the Snoozed mailbox.
    pub fn insert_snooze(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        until: i64,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| insert_snooze_tx(tx, account_id, message_id, until))
    }

    /// Delete a message's snooze row. Idempotent (no row → no-op). Called by
    /// `message.unsnooze` + the scheduler auto-return.
    pub fn delete_snooze(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), StoreError> {
        self.write_transaction(|tx| delete_snooze_tx(tx, account_id, message_id))
    }

    /// Messages whose return time has arrived (`until <= now`), for the
    /// scheduler tick. Returns `(message_id, until)` pairs.
    pub fn list_due_snoozes(
        &self,
        account_id: &AccountId,
        now: i64,
    ) -> Result<Vec<(MessageId, i64)>, StoreError> {
        let connection = self.read_connection()?;
        list_due_snoozes(&connection, account_id, now)
    }
}

impl posthaste_domain_service::SnoozeStore for DatabaseStore {
    fn insert_snooze(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        until: i64,
    ) -> Result<(), StoreError> {
        DatabaseStore::insert_snooze(self, account_id, message_id, until)
    }

    fn delete_snooze(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), StoreError> {
        DatabaseStore::delete_snooze(self, account_id, message_id)
    }

    fn list_due_snoozes(
        &self,
        account_id: &AccountId,
        now: i64,
    ) -> Result<Vec<(MessageId, i64)>, StoreError> {
        DatabaseStore::list_due_snoozes(self, account_id, now)
    }
}

pub(crate) fn insert_snooze_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    until: i64,
) -> Result<(), StoreError> {
    tx.execute_cached(
        "INSERT INTO message_snooze (account_id, message_id, until)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(account_id, message_id) DO UPDATE SET until = excluded.until",
        params![account_id.as_str(), message_id.as_str(), until],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

pub(crate) fn delete_snooze_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<(), StoreError> {
    tx.execute_cached(
        "DELETE FROM message_snooze WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

/// The store invariant: whenever a message's mailboxes are replaced, clear its
/// snooze row. The snooze mutation re-inserts the row after the move, so this
/// only affects messages *leaving* the Snoozed mailbox (by unsnooze, undo, a
/// manual move, or the scheduler's auto-return) — keeping the row from
/// orphaning when a snoozed message is moved out by any path. The sync path
/// does NOT route through `replace_mailboxes_tx`, so provider re-sync never
/// clobbers a snooze.
#[cfg(test)]
pub(crate) fn clear_snooze_on_mailbox_replace_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
) -> Result<(), StoreError> {
    tx.execute_cached(
        "DELETE FROM message_snooze WHERE account_id = ?1 AND message_id = ?2",
        params![account_id.as_str(), message_id.as_str()],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

fn list_due_snoozes(
    connection: &Connection,
    account_id: &AccountId,
    now: i64,
) -> Result<Vec<(MessageId, i64)>, StoreError> {
    let mut statement = connection
        .prepare(
            "SELECT message_id, until FROM message_snooze
             WHERE account_id = ?1 AND until <= ?2
             ORDER BY until ASC
             LIMIT ?3",
        )
        .map_err(sql_to_store_error)?;
    let rows = statement
        .query_map(
            params![account_id.as_str(), now, SNOOZE_DUE_BATCH_LIMIT],
            |row| Ok((MessageId(row.get::<_, String>(0)?), row.get::<_, i64>(1)?)),
        )
        .map_err(sql_to_store_error)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(sql_to_store_error)?);
    }
    Ok(out)
}
