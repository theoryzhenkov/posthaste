use super::*;
use crate::sql_cache::CachedSql;
use std::ops::Deref;

const MAX_IDLE_READ_CONNECTIONS: usize = 4;

/// SQLite-backed store with a single serialized write connection and pooled
/// read connections. Raw MIME bodies are stored as content-addressed files
/// on disk.
///
/// @spec docs/L1-sync#sqlite-schema
/// @spec docs/L0-accounts#the-invariant
pub struct DatabaseStore {
    db_path: PathBuf,
    data_root: PathBuf,
    write_connection: Mutex<Connection>,
    read_connections: Mutex<Vec<Connection>>,
}

pub(crate) struct ReadConnection<'store> {
    pool: &'store Mutex<Vec<Connection>>,
    connection: Option<Connection>,
}

impl Deref for ReadConnection<'_> {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.connection
            .as_ref()
            .expect("read connection available before drop")
    }
}

impl Drop for ReadConnection<'_> {
    fn drop(&mut self) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        if let Ok(mut pool) = self.pool.lock() {
            if pool.len() < MAX_IDLE_READ_CONNECTIONS {
                pool.push(connection);
            }
        }
    }
}

impl DatabaseStore {
    /// Opens (or creates) the SQLite database and data directory, runs schema
    /// migrations, and returns a ready-to-use store.
    pub fn open(
        db_path: impl Into<PathBuf>,
        data_root: impl Into<PathBuf>,
    ) -> Result<Self, StoreError> {
        let db_path = db_path.into();
        let data_root = data_root.into();
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).map_err(io_to_store_error)?;
        }
        fs::create_dir_all(&data_root).map_err(io_to_store_error)?;

        let connection =
            Connection::open(&db_path).map_err(|err| StoreError::Failure(err.to_string()))?;
        configure_connection(&connection)?;
        let mut connection = connection;
        init_schema(&mut connection)?;

        ph_info!(
            events::DATABASE_OPENED,
            db_path = %db_path.display(),
            "database store opened"
        );
        Ok(Self {
            db_path,
            data_root,
            write_connection: Mutex::new(connection),
            read_connections: Mutex::new(Vec::new()),
        })
    }

    /// Checks out a read SQLite connection (WAL mode allows concurrent readers).
    ///
    /// Read connections are pooled so hot read statements and SQLite page-cache
    /// state survive across UI queries. The connection is returned to the idle
    /// pool when the guard is dropped.
    pub(crate) fn read_connection(&self) -> Result<ReadConnection<'_>, StoreError> {
        let connection = {
            let mut pool = self
                .read_connections
                .lock()
                .map_err(|_| StoreError::Failure("read pool lock poisoned".to_string()))?;
            pool.pop()
        };
        let connection = match connection {
            Some(connection) => connection,
            None => {
                let connection = Connection::open(&self.db_path)
                    .map_err(|err| StoreError::Failure(err.to_string()))?;
                configure_connection(&connection)?;
                connection
            }
        };
        Ok(ReadConnection {
            pool: &self.read_connections,
            connection: Some(connection),
        })
    }

    /// Acquires the write lock and executes `operation` inside a single SQLite
    /// transaction. Rolls back on error.
    ///
    /// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
    pub(crate) fn write_transaction<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut connection = self
            .write_connection
            .lock()
            .map_err(|_| StoreError::Failure("write lock poisoned".to_string()))?;
        let tx = connection
            .transaction()
            .map_err(|err| StoreError::Failure(err.to_string()))?;
        let result = operation(&tx)?;
        tx.commit()
            .map_err(|err| StoreError::Failure(err.to_string()))?;
        Ok(result)
    }

    /// Writes raw MIME to a content-addressed file under `data_root/accounts/
    /// {account_id}/messages/{sha256_prefix}/{sha256}.eml`. Deduplicates by
    /// hash.
    pub(crate) fn store_raw_message(
        &self,
        account_id: &AccountId,
        raw_mime: &str,
    ) -> Result<RawMessageRef, StoreError> {
        let mut hasher = Sha256::new();
        hasher.update(raw_mime.as_bytes());
        let sha256 = hex_encode(hasher.finalize());
        let prefix = &sha256[..2];
        let directory = self
            .data_root
            .join("accounts")
            .join(account_id.as_str())
            .join("messages")
            .join(prefix);
        fs::create_dir_all(&directory).map_err(io_to_store_error)?;
        let path = directory.join(format!("{sha256}.eml"));
        if !path.exists() {
            fs::write(&path, raw_mime).map_err(io_to_store_error)?;
        }
        Ok(RawMessageRef {
            path: path.to_string_lossy().to_string(),
            sha256,
            size: raw_mime.len() as i64,
            mime_type: "message/rfc822".to_string(),
            fetched_at: now_iso8601()?,
        })
    }

    /// Persists a sync state token in the same transaction as sync data.
    ///
    /// @spec docs/L1-sync#state-management
    pub(crate) fn upsert_sync_cursor_tx(
        tx: &Transaction<'_>,
        account_id: &AccountId,
        cursor: &SyncCursor,
    ) -> Result<(), StoreError> {
        tx.execute_cached(
            "INSERT INTO sync_cursor (account_id, object_type, state, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(account_id, object_type) DO UPDATE SET
                state = excluded.state,
                updated_at = excluded.updated_at",
            params![
                account_id.as_str(),
                cursor.object_type.as_str(),
                cursor.state,
                cursor.updated_at
            ],
        )
        .map_err(sql_to_store_error)?;
        Ok(())
    }
}
