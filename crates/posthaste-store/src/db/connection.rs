use super::*;
use regex::Regex;
use rusqlite::functions::FunctionFlags;
use std::sync::Arc;
use std::time::Duration;

const SQLITE_CACHE_SIZE_KIB: i64 = -65_536;
// Memory-mapped I/O is disabled. An mmap'd write that cannot complete (disk
// pressure, the file changing under another process, an I/O error) bypasses
// SQLite's normal error handling and can tear a page, corrupting the database
// ("database disk image is malformed"). The page cache above already covers the
// read-heavy workload; mmap is not worth the corruption risk for a local cache.
const SQLITE_MMAP_SIZE_BYTES: i64 = 0;

/// Configures WAL journal mode, foreign-key enforcement, and read-heavy cache tuning.
pub(crate) fn configure_connection(connection: &Connection) -> Result<(), StoreError> {
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(sql_to_store_error)?;
    connection
        .pragma_update(None, "journal_mode", "wal")
        .map_err(sql_to_store_error)?;
    connection
        .pragma_update(None, "synchronous", "NORMAL")
        .map_err(sql_to_store_error)?;
    connection
        .pragma_update(None, "cache_size", SQLITE_CACHE_SIZE_KIB)
        .map_err(sql_to_store_error)?;
    connection
        .pragma_update(None, "temp_store", "MEMORY")
        .map_err(sql_to_store_error)?;
    connection
        .pragma_update(None, "mmap_size", SQLITE_MMAP_SIZE_BYTES)
        .map_err(sql_to_store_error)?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(sql_to_store_error)?;
    // Hold hot read/write statements (see `sql_cache`) without LRU eviction.
    connection.set_prepared_statement_cache_capacity(256);
    register_regexp_function(connection)?;
    Ok(())
}

/// Registers the `regexp(pattern, text)` scalar so the smart-mailbox `regex`
/// operator's `text REGEXP ?` clause works — SQLite ships no built-in REGEXP.
///
/// The `text REGEXP pattern` operator invokes `regexp(pattern, text)`. The
/// compiled [`Regex`] is cached per prepared-statement via `get_or_create_aux`
/// (keyed on the pattern argument, which is a bound constant per query), so a
/// pattern compiles once per statement, not once per row.
///
/// A pattern that fails to compile surfaces as a `rusqlite` error → `StoreError`,
/// never a panic — but in practice the write boundary (`validate_condition`)
/// already rejects a malformed pattern with the same `regex` engine, so an
/// un-compilable pattern never reaches a query.
fn register_regexp_function(connection: &Connection) -> Result<(), StoreError> {
    connection
        .create_scalar_function(
            "regexp",
            2,
            FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
            |ctx| {
                let regexp: Arc<Regex> = ctx.get_or_create_aux(
                    0,
                    |value| -> Result<_, Box<dyn std::error::Error + Send + Sync + 'static>> {
                        Ok(Regex::new(value.as_str()?)?)
                    },
                )?;
                // A NULL text side (e.g. a NULL column) never matches.
                let text = ctx.get_raw(1).as_str_or_null()?;
                Ok(text.is_some_and(|text| regexp.is_match(text)))
            },
        )
        .map_err(sql_to_store_error)
}

/// Returns the current time as an ISO 8601 string.
pub(crate) fn now_iso8601() -> Result<String, StoreError> {
    domain_now_iso8601().map_err(StoreError::Failure)
}

/// Parses a `sync_cursor.object_type` string into a `SyncObject` enum.
pub(crate) fn parse_sync_object(value: &str) -> Result<SyncObject, rusqlite::Error> {
    match value {
        "mailbox" => Ok(SyncObject::Mailbox),
        "message" => Ok(SyncObject::Message),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown sync object {other}"),
            )),
        )),
    }
}

/// Converts a bool to SQLite integer (0/1).
pub(crate) fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

/// Wraps a rusqlite error into a `StoreError`, distinguishing database
/// corruption so callers can offer a repair pathway.
pub(crate) fn sql_to_store_error(err: rusqlite::Error) -> StoreError {
    if is_corruption_error(&err) {
        StoreError::Corruption(err.to_string())
    } else {
        StoreError::Failure(err.to_string())
    }
}

/// Returns true when a rusqlite error indicates a corrupt or non-database file
/// (`SQLITE_CORRUPT` / `SQLITE_NOTADB`).
pub(crate) fn is_corruption_error(err: &rusqlite::Error) -> bool {
    use rusqlite::ErrorCode;
    matches!(
        err,
        rusqlite::Error::SqliteFailure(e, _)
            if matches!(e.code, ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
    )
}

/// Wraps an I/O error into `StoreError::Failure`.
pub(crate) fn io_to_store_error(err: std::io::Error) -> StoreError {
    StoreError::Failure(err.to_string())
}

/// Wraps a JSON serialization error into `StoreError::Failure`.
pub(crate) fn json_to_store_error(err: impl std::error::Error) -> StoreError {
    StoreError::Failure(err.to_string())
}
