//! Phase 2 undo/redo: the per-account server-authoritative reversible-op log
//! (`rev_log`) + cursor (`rev_cursor`) store layer.
//!
//! The log is append-only on forward actions; the cursor is mutable (undo/redo
//! move it). `diff` is a `MessageChangeDiff` JSON, opaque to the store — the
//! semantics live in `posthaste-link-core` / the client. The client proposes
//! idempotent cursor moves; the server arbitrates.
//!
//! @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract

use super::*;

/// The conventional per-account history cap (carried from Phase 1).
pub const MAX_REV_LOG_HISTORY: u32 = 50;

// `RevLogStep` + `RevCursor` live in `posthaste-domain` (shared with the
// `BackendApi` read channel + the `RevLog` synced view); `use super::*`
// brings them into scope via the crate re-export.

const REV_LOG_COLUMNS: &str = "step_id, seq, message_id, source_id, diff, created_at";

fn row_to_step(row: &Row) -> rusqlite::Result<RevLogStep> {
    let step_id: String = row.get(0)?;
    let seq: i64 = row.get(1)?;
    let message_id: String = row.get(2)?;
    let source_id: String = row.get(3)?;
    let diff_str: String = row.get(4)?;
    let created_at: String = row.get(5)?;
    let diff: Value = serde_json::from_str(&diff_str).unwrap_or(Value::Null);
    Ok(RevLogStep {
        step_id,
        seq: seq.max(0) as u32,
        message_id,
        source_id,
        diff,
        created_at,
    })
}

impl DatabaseStore {
    /// Append a reversible-op step. Idempotent on `step_id` (re-delivery of the
    /// same forward action is a no-op that returns the existing `seq`).
    /// Otherwise assigns `seq = MAX(seq) + 1` for the account + inserts.
    pub fn append_rev_log_step(
        &self,
        account_id: &AccountId,
        step_id: &str,
        message_id: &str,
        source_id: &str,
        diff: &Value,
        created_at: &str,
    ) -> Result<u32, StoreError> {
        let diff_str = serde_json::to_string(diff)
            .map_err(|e| StoreError::Failure(format!("invalid rev_log diff: {e}")))?;
        self.write_transaction(|tx| {
            // Idempotent: a re-delivered step_id returns its existing seq.
            let existing: Option<i64> = tx
                .query_row(
                    "SELECT seq FROM rev_log WHERE account_id = ?1 AND step_id = ?2",
                    params![account_id.as_str(), step_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sql_to_store_error)?;
            if let Some(seq) = existing {
                return Ok(seq.max(0) as u32);
            }
            let seq: i64 = tx
                .query_row(
                    "SELECT COALESCE(MAX(seq), 0) + 1 FROM rev_log WHERE account_id = ?1",
                    params![account_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(sql_to_store_error)?;
            tx.execute(
                "INSERT INTO rev_log (account_id, step_id, seq, message_id, source_id, diff, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    account_id.as_str(),
                    step_id,
                    seq,
                    message_id,
                    source_id,
                    diff_str,
                    created_at,
                ],
            )
            .map_err(sql_to_store_error)?;
            Ok(seq.max(0) as u32)
        })
    }

    /// Fetch the account's log ordered by `seq`. `since_seq` filters to steps
    /// with `seq > since_seq` (the sync delta); `None` fetches all. Capped by
    /// `limit`.
    pub fn fetch_rev_log(
        &self,
        account_id: &AccountId,
        since_seq: Option<u32>,
        limit: u32,
    ) -> Result<Vec<RevLogStep>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = match since_seq {
            Some(_) => connection
                .prepare(&format!(
                    "SELECT {REV_LOG_COLUMNS} FROM rev_log \
                     WHERE account_id = ?1 AND seq > ?2 ORDER BY seq ASC LIMIT ?3"
                ))
                .map_err(sql_to_store_error)?,
            None => connection
                .prepare(&format!(
                    "SELECT {REV_LOG_COLUMNS} FROM rev_log \
                     WHERE account_id = ?1 ORDER BY seq ASC LIMIT ?2"
                ))
                .map_err(sql_to_store_error)?,
        };
        let rows = match since_seq {
            Some(since) => statement
                .query_map(params![account_id.as_str(), since, limit], row_to_step)
                .map_err(sql_to_store_error)?,
            None => statement
                .query_map(params![account_id.as_str(), limit], row_to_step)
                .map_err(sql_to_store_error)?,
        };
        let mut steps = Vec::new();
        for row in rows {
            steps.push(row.map_err(sql_to_store_error)?);
        }
        Ok(steps)
    }

    /// The account's cursor. Defaults to `{ None, [] }` if no row exists.
    pub fn get_rev_cursor(&self, account_id: &AccountId) -> Result<RevCursor, StoreError> {
        let connection = self.read_connection()?;
        let row: Option<(Option<String>, String)> = connection
            .query_row(
                "SELECT cursor_step_id, redo_tail FROM rev_cursor WHERE account_id = ?1",
                params![account_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(sql_to_store_error)?;
        match row {
            None => Ok(RevCursor::default()),
            Some((cursor_step_id, redo_tail_str)) => Ok(RevCursor {
                cursor_step_id,
                redo_tail: serde_json::from_str(&redo_tail_str).unwrap_or_default(),
            }),
        }
    }

    /// Set the account's cursor (idempotent upsert). `cursor_step_id = None`
    /// means all undone.
    pub fn set_rev_cursor(
        &self,
        account_id: &AccountId,
        cursor_step_id: Option<&str>,
        redo_tail: &[String],
    ) -> Result<(), StoreError> {
        let redo_tail_str = serde_json::to_string(redo_tail)
            .map_err(|e| StoreError::Failure(format!("invalid rev_cursor redo_tail: {e}")))?;
        self.write_transaction(|tx| {
            tx.execute(
                "INSERT INTO rev_cursor (account_id, cursor_step_id, redo_tail)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(account_id) DO UPDATE SET cursor_step_id = ?2, redo_tail = ?3",
                params![account_id.as_str(), cursor_step_id, redo_tail_str],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }

    /// Evict the oldest steps once the log exceeds `max_steps` (per-account,
    /// count-based). Returns the number of rows deleted. The cursor is not
    /// clamped here — eviction is sized so the undoable range stays reachable;
    /// a cursor referencing an evicted step (rare: undo-all-50 then a 51st
    /// action) is clamped by the arbitrator on read.
    pub fn evict_oldest_rev_log(
        &self,
        account_id: &AccountId,
        max_steps: u32,
    ) -> Result<u32, StoreError> {
        self.write_transaction(|tx| {
            let count: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM rev_log WHERE account_id = ?1",
                    params![account_id.as_str()],
                    |row| row.get(0),
                )
                .map_err(sql_to_store_error)?;
            if count <= max_steps as i64 {
                return Ok(0);
            }
            let excess = count - max_steps as i64;
            let deleted = tx
                .execute(
                    "DELETE FROM rev_log WHERE account_id = ?1 AND seq IN (
                        SELECT seq FROM rev_log WHERE account_id = ?1 ORDER BY seq ASC LIMIT ?2
                    )",
                    params![account_id.as_str(), excess],
                )
                .map_err(sql_to_store_error)?;
            Ok(deleted.max(0) as u32)
        })
    }
}

impl RevLogStore for DatabaseStore {
    /// The account's `rev_log` steps + cursor — the snapshot behind the `RevLog`
    /// synced view. Combines the bounded log read with the cursor.
    fn rev_log_snapshot(&self, account_id: &AccountId) -> Result<RevLogSnapshot, StoreError> {
        let steps = self.fetch_rev_log(account_id, None, MAX_REV_LOG_HISTORY)?;
        let cursor = self.get_rev_cursor(account_id)?;
        Ok(RevLogSnapshot { steps, cursor })
    }

    /// Delegates to the inherent `DatabaseStore::append_rev_log_step` (method-call
    /// resolution prefers the inherent over the trait method of the same name).
    fn append_rev_log_step(
        &self,
        account_id: &AccountId,
        step_id: &str,
        message_id: &str,
        source_id: &str,
        diff: &serde_json::Value,
        created_at: &str,
    ) -> Result<u32, StoreError> {
        self.append_rev_log_step(account_id, step_id, message_id, source_id, diff, created_at)
    }
}
