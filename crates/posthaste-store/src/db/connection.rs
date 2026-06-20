use super::*;
use std::time::Duration;

const SQLITE_CACHE_SIZE_KIB: i64 = -65_536;
const SQLITE_MMAP_SIZE_BYTES: i64 = 256 * 1024 * 1024;

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
    Ok(())
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

/// Wraps a rusqlite error into `StoreError::Failure`.
pub(crate) fn sql_to_store_error(err: rusqlite::Error) -> StoreError {
    StoreError::Failure(err.to_string())
}

/// Wraps an I/O error into `StoreError::Failure`.
pub(crate) fn io_to_store_error(err: std::io::Error) -> StoreError {
    StoreError::Failure(err.to_string())
}

/// Wraps a JSON serialization error into `StoreError::Failure`.
pub(crate) fn json_to_store_error(err: impl std::error::Error) -> StoreError {
    StoreError::Failure(err.to_string())
}
