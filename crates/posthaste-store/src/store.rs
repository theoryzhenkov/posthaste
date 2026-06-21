use super::*;
use crate::sql_cache::CachedSql;
use std::ops::Deref;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_IDLE_READ_CONNECTIONS: usize = 4;

/// Marker file (under the data root) that forces a database rebuild on the next
/// open, used by the manual "repair local database" action.
pub const REPAIR_MARKER_FILE: &str = ".repair-requested";

/// Outcome of an automatic database repair performed during [`DatabaseStore::open_with_repair`].
#[derive(Clone, Debug)]
pub struct RepairReport {
    /// Path the corrupt database files were moved to.
    pub quarantined_path: PathBuf,
    /// Human-readable reason the repair was triggered.
    pub reason: String,
}

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
    /// Opens (or creates) the store, auto-repairing a corrupt database.
    ///
    /// Equivalent to [`DatabaseStore::open_with_repair`] but discards the repair
    /// report. Prefer `open_with_repair` where the caller can surface the repair.
    pub fn open(
        db_path: impl Into<PathBuf>,
        data_root: impl Into<PathBuf>,
    ) -> Result<Self, StoreError> {
        Self::open_with_repair(db_path, data_root).map(|(store, _)| store)
    }

    /// Opens (or creates) the store, quarantining and rebuilding the database
    /// when it is corrupt or a repair was requested via the marker file.
    ///
    /// The SQLite database is a rebuildable projection (accounts live in config,
    /// secrets in the keychain), so a corrupt file is moved aside and recreated
    /// rather than blocking launch. Returns a [`RepairReport`] when a repair
    /// happened so the caller can notify the user and trigger a re-sync.
    pub fn open_with_repair(
        db_path: impl Into<PathBuf>,
        data_root: impl Into<PathBuf>,
    ) -> Result<(Self, Option<RepairReport>), StoreError> {
        let db_path = db_path.into();
        let data_root = data_root.into();
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).map_err(io_to_store_error)?;
        }
        fs::create_dir_all(&data_root).map_err(io_to_store_error)?;

        let marker = data_root.join(REPAIR_MARKER_FILE);
        let repair_requested = marker.exists();

        let repair_reason = if repair_requested {
            Some("manual repair requested".to_string())
        } else {
            match Self::try_open(&db_path, &data_root) {
                Ok(store) => return Ok((store, None)),
                Err(StoreError::Corruption(reason)) => Some(reason),
                Err(other) => return Err(other),
            }
        };
        let reason = repair_reason.expect("repair reason set on the repair path");

        let quarantined_path = quarantine_database(&db_path)?;
        let store = Self::try_open(&db_path, &data_root)?;
        if repair_requested {
            let _ = fs::remove_file(&marker);
        }
        ph_warn!(
            events::DATABASE_CORRUPT_REPAIRED,
            db_path = %db_path.display(),
            quarantined = %quarantined_path.display(),
            reason = %reason,
            "database quarantined and rebuilt"
        );
        Ok((
            store,
            Some(RepairReport {
                quarantined_path,
                reason,
            }),
        ))
    }

    fn try_open(db_path: &Path, data_root: &Path) -> Result<Self, StoreError> {
        let connection = Connection::open(db_path).map_err(sql_to_store_error)?;
        configure_connection(&connection)?;
        let mut connection = connection;
        init_schema(&mut connection)?;

        ph_info!(
            events::DATABASE_OPENED,
            db_path = %db_path.display(),
            "database store opened"
        );
        Ok(Self {
            db_path: db_path.to_path_buf(),
            data_root: data_root.to_path_buf(),
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

/// Moves the database and its WAL/SHM siblings aside to `<name>.corrupt-<unix>`.
///
/// Best effort: if a file cannot be renamed it is removed so a fresh database
/// can be created in its place. A rebuildable projection prefers a clean restart
/// over refusing to launch.
fn quarantine_database(db_path: &Path) -> Result<PathBuf, StoreError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let base_name = db_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("mail.sqlite");
    let quarantined = db_path.with_file_name(format!("{base_name}.corrupt-{timestamp}"));

    for suffix in ["", "-wal", "-shm"] {
        let from = sibling_path(db_path, suffix);
        if !from.exists() {
            continue;
        }
        let to = sibling_path(&quarantined, suffix);
        if fs::rename(&from, &to).is_err() {
            let _ = fs::remove_file(&from);
        }
    }
    Ok(quarantined)
}

/// Returns the path of a SQLite sibling file (`""`, `-wal`, `-shm`).
fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    if suffix.is_empty() {
        return path.to_path_buf();
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    path.with_file_name(format!("{name}{suffix}"))
}
