use super::*;
use crate::sql_cache::CachedSql;
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, Condvar};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_IDLE_READ_CONNECTIONS: usize = 4;

/// Hard cap on *concurrently open* read connections (idle-pooled + checked
/// out), enforced by [`ConnectionSemaphore`]. Previously only the idle pool
/// was bounded (`MAX_IDLE_READ_CONNECTIONS`): once it was empty, every
/// additional concurrent reader opened a brand-new `Connection` with no limit
/// at all — N concurrent readers meant N simultaneous SQLite file handles,
/// uncapped (N16 / RFC-L2-lifecycle D67(c) / M27 sub-unit (c)). Must be `>=
/// MAX_IDLE_READ_CONNECTIONS` (an idle-pooled connection holds its permit for
/// as long as it stays in the pool). **Review** (picked sane, not measured).
pub(crate) const MAX_READ_CONNECTIONS: usize = 16;

/// Minimal blocking counting semaphore gating SQLite read-connection
/// creation (N16). `DatabaseStore::read_connection` is a plain synchronous
/// function reachable both from the tokio blocking pool (`read_async`,
/// `write_transaction_async`) and from ordinary `#[test]` functions with no
/// tokio runtime at all — so an async `tokio::sync::Semaphore::acquire()` is
/// not usable here. This blocks the calling *thread* (not a task) until a
/// permit is available: fine on the blocking pool (that's what it's for) and
/// fine in a plain sync test (no runtime to starve).
struct ConnectionSemaphore {
    available: Mutex<usize>,
    condvar: Condvar,
}

impl ConnectionSemaphore {
    fn new(permits: usize) -> Self {
        Self {
            available: Mutex::new(permits),
            condvar: Condvar::new(),
        }
    }

    /// Blocks the calling thread until a permit is free, then takes it.
    /// Waiters queue: every [`ConnectionPermit`] release wakes exactly one
    /// waiter, so nobody spins and nobody starves.
    ///
    /// Takes `&Arc<Self>` (rather than `self: &Arc<Self>`, an unstable
    /// receiver type on stable Rust) so the returned [`ConnectionPermit`] can
    /// hold its own owned `Arc` clone and release independently of `self`'s
    /// borrow.
    fn acquire(semaphore: &Arc<Self>) -> ConnectionPermit {
        let mut available = match semaphore.available.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        while *available == 0 {
            available = match semaphore.condvar.wait(available) {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
        *available -= 1;
        ConnectionPermit {
            semaphore: semaphore.clone(),
        }
    }
}

/// RAII permit from [`ConnectionSemaphore::acquire`]. Releasing (drop) is
/// what lets the next waiter — if any — proceed, so this must be held for the
/// full lifetime of the connection it was acquired for, including while that
/// connection sits idle in the pool (see [`ReadConnection`]), not just while
/// checked out.
struct ConnectionPermit {
    semaphore: Arc<ConnectionSemaphore>,
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        let mut available = match self.semaphore.available.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *available += 1;
        drop(available);
        self.semaphore.condvar.notify_one();
    }
}

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
    /// `None` once [`DatabaseStore::close`] has run: the write connection is
    /// dropped on close so its file handle is released and any post-close write
    /// fails cleanly (`StoreError::Failure`) instead of touching a checkpointed,
    /// about-to-be-gone database.
    write_connection: Mutex<Option<Connection>>,
    /// Idle-pooled read connections, each paired with the [`ConnectionPermit`]
    /// it was created under. The permit travels with the connection while it
    /// sits idle in this pool — it is only released when the connection is
    /// actually closed (evicted here, or on [`DatabaseStore::close`]), which
    /// is what makes [`MAX_READ_CONNECTIONS`] a true peak-concurrency bound
    /// rather than just a bound on the idle pool (N16).
    read_connections: Mutex<Vec<(Connection, ConnectionPermit)>>,
    read_connection_limiter: Arc<ConnectionSemaphore>,
}

pub(crate) struct ReadConnection<'store> {
    pool: &'store Mutex<Vec<(Connection, ConnectionPermit)>>,
    connection: Option<Connection>,
    permit: Option<ConnectionPermit>,
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
        let (Some(connection), Some(permit)) = (self.connection.take(), self.permit.take()) else {
            return;
        };
        let mut pool = lock_read_pool(self.pool);
        if pool.len() < MAX_IDLE_READ_CONNECTIONS {
            pool.push((connection, permit));
        }
        // Else: `connection` and `permit` are dropped here — the file handle
        // closes and the concurrency slot frees for the next waiter.
    }
}

fn lock_read_pool(
    pool: &Mutex<Vec<(Connection, ConnectionPermit)>>,
) -> MutexGuard<'_, Vec<(Connection, ConnectionPermit)>> {
    match pool.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            ph_warn!(
                events::STORE_MUTEX_POISONED,
                mutex = "read_pool",
                "read connection pool mutex was poisoned; recovering"
            );
            poisoned.into_inner()
        }
    }
}

fn lock_write_connection(
    connection: &Mutex<Option<Connection>>,
) -> MutexGuard<'_, Option<Connection>> {
    match connection.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            ph_warn!(
                events::STORE_MUTEX_POISONED,
                mutex = "write_connection",
                "write connection mutex was poisoned; recovering"
            );
            poisoned.into_inner()
        }
    }
}

/// RAII guard over the content-addressed body (`.eml`) files staged to disk
/// *before* the enclosing write transaction commits.
///
/// Raw MIME bodies are written to disk ahead of the txn (dedup by hash, and to
/// keep large blobs off the write lock — see [`DatabaseStore::store_raw_message`]).
/// A rolled-back txn would otherwise leave those files on disk with no
/// referencing row — the N14 orphan-`.eml` leak. This guard records every
/// *newly written* path and, unless [`Self::commit`] is called after the txn
/// commits, removes them on drop. Deduped hits (the file already existed) are
/// never registered, so a rollback only ever removes what this operation wrote.
///
/// @spec docs/eph/RFC-L2-lifecycle-and-errors#d62
pub(crate) struct StagedBodyFiles {
    paths: Vec<PathBuf>,
    committed: bool,
}

impl StagedBodyFiles {
    pub(crate) fn new() -> Self {
        Self {
            paths: Vec::new(),
            committed: false,
        }
    }

    /// Record a body file this operation just wrote, so it is removed if the
    /// enclosing txn does not commit.
    fn register(&mut self, path: PathBuf) {
        self.paths.push(path);
    }

    /// Disarm the guard: the enclosing txn committed, so the staged files are
    /// now referenced and must survive.
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for StagedBodyFiles {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for path in &self.paths {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => ph_warn!(
                    events::STORE_CACHE_STAGED_BODY_REMOVE_FAILED,
                    path = %path.display(),
                    error = %err,
                    "failed to remove orphaned staged body"
                ),
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
        prepare_schema(&mut connection)?;

        ph_info!(
            events::DATABASE_OPENED,
            db_path = %db_path.display(),
            "database store opened"
        );
        Ok(Self {
            db_path: db_path.to_path_buf(),
            data_root: data_root.to_path_buf(),
            write_connection: Mutex::new(Some(connection)),
            read_connections: Mutex::new(Vec::new()),
            read_connection_limiter: Arc::new(ConnectionSemaphore::new(MAX_READ_CONNECTIONS)),
        })
    }

    /// Close the store cleanly as the final teardown step (D62 / M20 phase (c)).
    ///
    /// Releases every pooled read connection, checkpoints the WAL back into the
    /// main database file (`PRAGMA wal_checkpoint(TRUNCATE)`), then drops the
    /// write connection so all SQLite file handles are released promptly and any
    /// post-close write fails cleanly (`StoreError::Failure`).
    ///
    /// **Idempotent:** once the write connection has been taken, a second call
    /// is a no-op.
    ///
    /// **Deadline:** bounded by the sequence's ~2s store-close phase (RFC §7
    /// ruling 1). It aims to complete, not to enforce — the phase owns the
    /// deadline. A checkpoint that cannot acquire its locks (a straggling
    /// reader) or otherwise fails is logged and skipped: a missed checkpoint
    /// costs a WAL replay on next open, not data.
    ///
    /// @spec docs/eph/RFC-L2-lifecycle-and-errors#d62
    pub fn close(&self) {
        // Release pooled read connections first so the TRUNCATE checkpoint below
        // is not blocked by an idle reader still holding the WAL open.
        {
            let mut pool = lock_read_pool(&self.read_connections);
            pool.clear();
        }

        let mut guard = lock_write_connection(&self.write_connection);
        if let Some(connection) = guard.as_ref() {
            // Flush the WAL back into the main db file and truncate it to zero,
            // so a clean shutdown leaves no WAL to replay on next open (N3).
            if let Err(err) = connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
                ph_warn!(
                    events::STORE_CLOSE_WAL_CHECKPOINT_FAILED,
                    error = %err,
                    "store close: wal_checkpoint(TRUNCATE) failed"
                );
            }
        }
        // Drop the write connection: releases its file handle now and makes any
        // subsequent write error cleanly rather than resurrecting the WAL.
        *guard = None;
    }

    /// Checks out a read SQLite connection (WAL mode allows concurrent readers).
    ///
    /// Read connections are pooled so hot read statements and SQLite page-cache
    /// state survive across UI queries. The connection is returned to the idle
    /// pool when the guard is dropped.
    ///
    /// The *peak* number of simultaneously open read connections — idle-pooled
    /// plus checked-out — is bounded by [`MAX_READ_CONNECTIONS`]
    /// (`read_connection_limiter`), not just the idle pool
    /// (`MAX_IDLE_READ_CONNECTIONS`): when the idle pool is empty and the
    /// limiter is already at capacity, this call blocks the calling thread
    /// until an existing connection is released, rather than opening an
    /// unbounded new one (N16).
    pub(crate) fn read_connection(&self) -> Result<ReadConnection<'_>, StoreError> {
        let pooled = {
            let mut pool = lock_read_pool(&self.read_connections);
            pool.pop()
        };
        let (connection, permit) = match pooled {
            Some(pooled) => pooled,
            None => {
                let permit = ConnectionSemaphore::acquire(&self.read_connection_limiter);
                let connection = Connection::open(&self.db_path)
                    .map_err(|err| StoreError::Failure(err.to_string()))?;
                configure_connection(&connection)?;
                (connection, permit)
            }
        };
        Ok(ReadConnection {
            pool: &self.read_connections,
            connection: Some(connection),
            permit: Some(permit),
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
        let mut guard = lock_write_connection(&self.write_connection);
        let connection = guard
            .as_mut()
            .ok_or_else(|| StoreError::Failure("store is closed".to_string()))?;
        let tx = connection
            .transaction()
            .map_err(|err| StoreError::Failure(err.to_string()))?;
        let result = operation(&tx)?;
        tx.commit()
            .map_err(|err| StoreError::Failure(err.to_string()))?;
        Ok(result)
    }

    /// Repairs missing/orphaned body-cache bookkeeping rows (`cache_object`,
    /// `cache_rescore_queue`, `cache_message_signal`) via three correlated
    /// `NOT EXISTS` scans against `message`, catching up any rows a prior
    /// interrupted write left inconsistent.
    ///
    /// This used to run unconditionally inside `init_schema`, on every open,
    /// blocking `DatabaseStore::open`'s return — and therefore every first
    /// read/write — behind an unbounded full-table scan (N15 /
    /// RFC-L2-lifecycle D67(b) / M27 sub-unit (b)). It no longer runs on that
    /// path: the composition root (`build_authority_server_parts`) now calls
    /// this once, as a deferred post-startup task, after the store is already
    /// open and serving real reads/writes. A store that is never repaired
    /// (this call never runs, or runs after the process exits mid-way) is
    /// self-healing: the next call — on the next startup, or a future retry
    /// — repairs whatever is still missing, since the scans are idempotent
    /// (`NOT EXISTS`, not "since last repair").
    ///
    /// @spec docs/eph/RFC-L2-lifecycle-and-errors#d67
    pub fn repair_body_cache_objects(&self) -> Result<(), StoreError> {
        let mut guard = lock_write_connection(&self.write_connection);
        let connection = guard
            .as_mut()
            .ok_or_else(|| StoreError::Failure("store is closed".to_string()))?;
        crate::cache::repair_missing_body_cache_objects(connection)
    }

    /// Async offload of [`Self::write_transaction`]: runs the blocking rusqlite
    /// write transaction on the tokio **blocking pool** via
    /// [`tokio::task::spawn_blocking`], so SQLite work — the lock acquire, the
    /// txn, `serde`/param building inside `operation` — never occupies an async
    /// worker thread (D63 / audit N4). This is the write choke point every
    /// runtime write should offload through.
    ///
    /// The write `Mutex` is `std::sync` and is acquired *inside* the blocking
    /// closure (by `write_transaction`), never held across an `.await` — the
    /// guard lives and dies on one blocking thread. `rusqlite::Connection` is
    /// `Send`, and the store is moved in as `Arc<Self>` (`Send + Sync +
    /// 'static`), so the guard travels into the closure by the standard pattern.
    /// A panic inside `operation` is caught by `spawn_blocking` and surfaced as
    /// a `StoreError::Failure` (the [`StagedBodyFiles`] guard, if any, still runs
    /// its `Drop` during the unwind on the blocking thread — panic-safe across
    /// the pool boundary).
    ///
    /// @spec docs/eph/RFC-L2-lifecycle-and-errors#d63
    pub async fn write_transaction_async<T>(
        self: Arc<Self>,
        operation: impl FnOnce(&Transaction<'_>) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError>
    where
        T: Send + 'static,
    {
        tokio::task::spawn_blocking(move || self.write_transaction(operation))
            .await
            .unwrap_or_else(|join_err| {
                Err(StoreError::Failure(format!(
                    "store write task failed: {join_err}"
                )))
            })
    }

    /// Async offload of a pooled read: acquires a read connection and runs
    /// `query` on the tokio **blocking pool** via
    /// [`tokio::task::spawn_blocking`] (D63 / audit N4). WAL mode lets this run
    /// concurrently with an in-flight write transaction, so a slow write no
    /// longer blocks a concurrent read on the async runtime.
    ///
    /// The read connection is checked out and returned to the pool entirely
    /// within the blocking closure; a panic is caught and surfaced as a
    /// `StoreError::Failure`.
    ///
    /// @spec docs/eph/RFC-L2-lifecycle-and-errors#d63
    pub async fn read_async<T>(
        self: Arc<Self>,
        query: impl FnOnce(&Connection) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError>
    where
        T: Send + 'static,
    {
        tokio::task::spawn_blocking(move || {
            let connection = self.read_connection()?;
            query(&connection)
        })
        .await
        .unwrap_or_else(|join_err| {
            Err(StoreError::Failure(format!(
                "store read task failed: {join_err}"
            )))
        })
    }

    /// Runs a write transaction whose content-addressed body files are staged to
    /// disk *before* the transaction, under a [`StagedBodyFiles`] guard (N14).
    ///
    /// `stage` writes the `.eml` files (registering each newly written one on
    /// the guard) and returns the staged refs; `apply` runs inside the txn using
    /// them. The guard is disarmed only once the txn commits — a staging error
    /// or a rolled-back txn drops it, removing the just-written files so no
    /// orphan `.eml` is left on disk. This is the single seam every body-staging
    /// mutation routes through.
    ///
    /// @spec docs/eph/RFC-L2-lifecycle-and-errors#d62
    pub(crate) fn staged_write<S, T>(
        &self,
        stage: impl FnOnce(&Self, &mut StagedBodyFiles) -> Result<S, StoreError>,
        apply: impl FnOnce(&Transaction<'_>, &S) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let mut staged = StagedBodyFiles::new();
        let staged_refs = stage(self, &mut staged)?;
        let result = self.write_transaction(|tx| apply(tx, &staged_refs))?;
        staged.commit();
        Ok(result)
    }

    /// Writes raw MIME to a content-addressed file under `data_root/accounts/
    /// {account_id}/messages/{sha256_prefix}/{sha256}.eml`. Deduplicates by
    /// hash.
    pub(crate) fn store_raw_message(
        &self,
        account_id: &AccountId,
        raw_mime: &str,
        staged: &mut StagedBodyFiles,
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
            // Register only newly written files: a dedup hit belongs to an
            // already-committed row and must survive a rollback of *this* txn.
            staged.register(path.clone());
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
