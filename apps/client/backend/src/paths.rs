//! Filesystem layout: the config root (TOML config repository), the state
//! root (SQLite store + body files), and the connection-info file the API
//! layer writes for local clients to discover the port and session secret.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File name of the connection-info document inside the state root.
const CONNECTION_INFO_FILE: &str = "connection-info.json";

/// Database file name inside the state root.
const DATABASE_FILE: &str = "mail.sqlite";

/// File name of the instance-lock file inside the state root.
const LOCK_FILE: &str = "backend.lock";

/// Resolved filesystem roots for one backend instance.
#[derive(Clone, Debug)]
pub struct AppPaths {
    /// TOML config repository root (accounts, smart mailboxes, app settings).
    pub config_root: PathBuf,
    /// State root: the SQLite database, content-addressed bodies, and the
    /// connection-info file.
    pub state_root: PathBuf,
}

impl AppPaths {
    /// Resolve roots through the canonical shared resolver in
    /// [`posthaste_config::paths`]: `POSTHASTE_CONFIG_ROOT` /
    /// `POSTHASTE_STATE_ROOT`, falling back to the XDG defaults
    /// (`$XDG_CONFIG_HOME/posthaste`, `$XDG_DATA_HOME/posthaste`) — the same
    /// directories every earlier release opened, so an existing install's
    /// data is found in place.
    pub fn resolve() -> Self {
        Self {
            config_root: posthaste_config::paths::config_root(),
            state_root: posthaste_config::paths::state_root(),
        }
    }

    /// Explicit roots (tests, embedding hosts).
    pub fn with_roots(config_root: impl Into<PathBuf>, state_root: impl Into<PathBuf>) -> Self {
        Self {
            config_root: config_root.into(),
            state_root: state_root.into(),
        }
    }

    /// Path of the SQLite database file.
    pub fn db_path(&self) -> PathBuf {
        self.state_root.join(DATABASE_FILE)
    }

    /// Path of the instance-lock file.
    pub fn lock_path(&self) -> PathBuf {
        self.state_root.join(LOCK_FILE)
    }

    /// Path of the connection-info file.
    pub fn connection_info_path(&self) -> PathBuf {
        self.state_root.join(CONNECTION_INFO_FILE)
    }
}

/// Exclusive advisory lock on the state root: one live backend per store.
///
/// SQLite in WAL mode happily lets a second process open the same database,
/// so nothing below this layer stops two backends (a second desktop launch,
/// another channel's build over the shared XDG roots, or the standalone
/// backend binary) from racing sync engines and outbox processors over one
/// store and clobbering each other's connection-info file. The lock is taken
/// before the database opens and released by [`AppState::shutdown`] (or when
/// the last state handle drops); the OS releases it when the holding process
/// exits, so a crash never leaves the store stuck. The lock file itself is
/// never deleted — removing it would race a concurrent acquire.
///
/// [`AppState`]: crate::AppState
/// [`AppState::shutdown`]: crate::AppState::shutdown
#[derive(Debug)]
pub struct StoreLock {
    /// The locked file, held open while the lock is live; closing it (via
    /// [`StoreLock::release`] or drop) releases the OS lock.
    file: std::sync::Mutex<Option<fs::File>>,
}

impl StoreLock {
    /// Try to take the exclusive lock, creating the lock file if needed.
    /// `Ok(None)` means another live backend holds it.
    pub fn acquire(path: &Path) -> io::Result<Option<Self>> {
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self {
                file: std::sync::Mutex::new(Some(file)),
            })),
            Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(error)) => Err(error),
        }
    }

    /// Release the lock while handles to it may still be alive, so an
    /// ordered shutdown frees the store for a successor process without
    /// waiting for every state clone to drop. Idempotent.
    pub fn release(&self) {
        self.file.lock().expect("store lock poisoned").take();
    }
}

/// The connection-info document: where the API listens and the session
/// secret local clients present. Possession of the file is the local trust
/// boundary, so it is written owner-readable only.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub port: u16,
    pub token: String,
}

impl ConnectionInfo {
    /// Mint a connection info with a fresh random session token.
    pub fn generate(port: u16) -> Self {
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        Self { port, token }
    }

    /// Write the document atomically (temp file + rename) with owner-only
    /// permissions, creating the parent directory if needed. The temp file
    /// is created 0600 before the token bytes land in it, so the secret is
    /// never on disk readable by another user, not even for a moment.
    pub fn write(&self, path: &Path) -> io::Result<()> {
        use std::io::Write;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_vec_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        // A leftover temp file from an interrupted write may carry stale
        // permissions; remove it so the create below owns the mode.
        match fs::remove_file(&tmp) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(&body)?;
        drop(file);
        fs::rename(&tmp, path)
    }

    /// Remove the document; missing is fine (already-removed on a re-entrant
    /// shutdown).
    pub fn remove(path: &Path) -> io::Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_info_round_trips_and_removes_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("connection-info.json");
        let info = ConnectionInfo::generate(4821);
        assert_eq!(info.token.len(), 64);

        info.write(&path).unwrap();
        let read: ConnectionInfo =
            serde_json::from_slice(&fs::read(&path).unwrap()).expect("parses back");
        assert_eq!(read.port, 4821);
        assert_eq!(read.token, info.token);

        ConnectionInfo::remove(&path).unwrap();
        ConnectionInfo::remove(&path).expect("second remove is a no-op");
    }

    #[test]
    fn store_lock_is_exclusive_and_releases_on_release_or_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backend.lock");

        let lock = StoreLock::acquire(&path)
            .unwrap()
            .expect("first acquire succeeds");
        assert!(
            StoreLock::acquire(&path).unwrap().is_none(),
            "second acquire must be refused while the lock is held"
        );

        lock.release();
        lock.release();
        let relocked = StoreLock::acquire(&path)
            .unwrap()
            .expect("the lock is free again after release");

        drop(relocked);
        assert!(
            StoreLock::acquire(&path).unwrap().is_some(),
            "the lock is free again after drop"
        );
    }
}
