//! Statement-cache helpers for the write path.
//!
//! Thin wrappers over `prepare_cached` that mirror `Connection::execute` and
//! `Connection::query_row`, so hot per-row loops reuse prepared statements on
//! the long-lived write connection instead of re-parsing+re-planning the same
//! SQL on every iteration. The cache is keyed by SQL text and lives on the
//! underlying `Connection`, so it persists across transactions and batches.
//!
//! Most important on the persistent write connection. Pooled read connections
//! also benefit when stable read queries use `prepare_cached`.

use rusqlite::{Params, Row, Transaction};

pub(crate) trait CachedSql {
    /// Like `execute`, but reuses a cached prepared statement.
    fn execute_cached<P: Params>(&self, sql: &str, params: P) -> rusqlite::Result<usize>;

    /// Like `query_row`, but reuses a cached prepared statement.
    fn query_row_cached<T, P, F>(&self, sql: &str, params: P, f: F) -> rusqlite::Result<T>
    where
        P: Params,
        F: FnOnce(&Row<'_>) -> rusqlite::Result<T>;
}

impl CachedSql for Transaction<'_> {
    fn execute_cached<P: Params>(&self, sql: &str, params: P) -> rusqlite::Result<usize> {
        self.prepare_cached(sql)?.execute(params)
    }

    fn query_row_cached<T, P, F>(&self, sql: &str, params: P, f: F) -> rusqlite::Result<T>
    where
        P: Params,
        F: FnOnce(&Row<'_>) -> rusqlite::Result<T>,
    {
        self.prepare_cached(sql)?.query_row(params, f)
    }
}
