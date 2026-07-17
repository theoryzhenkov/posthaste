//! Filesystem layout: the config root (TOML config repository), the state
//! root (SQLite store + body files), and the connection-info file the API
//! layer writes for local clients to discover the port and session secret.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Application directory name used under the XDG base directories.
const APP_DIR_NAME: &str = "posthaste";

/// File name of the connection-info document inside the state root.
const CONNECTION_INFO_FILE: &str = "connection-info.json";

/// Database file name inside the state root.
const DATABASE_FILE: &str = "mail.sqlite";

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
    /// Resolve roots from `POSTHASTE_CONFIG_ROOT` / `POSTHASTE_STATE_ROOT`,
    /// falling back to the XDG defaults (`$XDG_CONFIG_HOME/posthaste`,
    /// `$XDG_DATA_HOME/posthaste`).
    pub fn resolve() -> Self {
        let config_root = std::env::var("POSTHASTE_CONFIG_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| xdg_dir("XDG_CONFIG_HOME", ".config").join(APP_DIR_NAME));
        let state_root = std::env::var("POSTHASTE_STATE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| xdg_dir("XDG_DATA_HOME", ".local/share").join(APP_DIR_NAME));
        Self {
            config_root,
            state_root,
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

    /// Path of the connection-info file.
    pub fn connection_info_path(&self) -> PathBuf {
        self.state_root.join(CONNECTION_INFO_FILE)
    }
}

/// Resolve an XDG base directory from its env var or `$HOME/{suffix}`.
fn xdg_dir(env_var: &str, fallback_suffix: &str) -> PathBuf {
    std::env::var(env_var)
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(fallback_suffix)
        })
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
}
