use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use posthaste_domain::{ConfigError, ConfigSnapshot};

use crate::defaults::default_smart_mailboxes;
use crate::schema::AppToml;

mod config_repository;
mod io;

use io::{
    io_error, load_snapshot_from_disk, lock_error, read_app_toml, write_app_toml,
    write_smart_mailbox_toml,
};

/// File-system-backed `ConfigRepository` that persists config as TOML files.
/// Keeps an in-memory `ConfigSnapshot` behind an `RwLock` so reads never hit
/// disk after initialization.
///
/// @spec docs/L1-accounts#configrepository-trait
pub struct TomlConfigRepository {
    pub(super) config_root: PathBuf,
    pub(super) snapshot: RwLock<ConfigSnapshot>,
}

impl TomlConfigRepository {
    /// Opens (or creates) the config directory and loads the initial snapshot
    /// from disk.
    ///
    /// @spec docs/L1-accounts#initialization
    pub fn open(config_root: impl Into<PathBuf>) -> Result<Self, ConfigError> {
        let config_root = config_root.into();
        fs::create_dir_all(&config_root).map_err(io_error)?;
        fs::create_dir_all(config_root.join("sources")).map_err(io_error)?;
        fs::create_dir_all(config_root.join("smart-mailboxes")).map_err(io_error)?;

        let snapshot = load_snapshot_from_disk(&config_root)?;
        Ok(Self {
            config_root,
            snapshot: RwLock::new(snapshot),
        })
    }

    /// Returns the root directory path for this config repository.
    pub fn config_root(&self) -> &Path {
        &self.config_root
    }

    /// Returns `true` if `app.toml` does not exist, indicating the repository
    /// has not been initialized.
    ///
    /// @spec docs/L1-accounts#initialization
    pub fn is_empty(&self) -> bool {
        !self.config_root.join("app.toml").exists()
    }

    /// Creates `app.toml` and writes the default smart mailboxes. Called on
    /// first launch when no config exists.
    ///
    /// @spec docs/L1-accounts#initialization
    pub fn initialize_defaults(&self) -> Result<(), ConfigError> {
        let app = AppToml {
            schema_version: 1,
            default_source_id: None,
            automations: Vec::new(),
            draft_automations: Vec::new(),
            daemon: Default::default(),
            logging: Default::default(),
            cache: Default::default(),
        };
        write_app_toml(&self.config_root, &app)?;

        for mailbox in default_smart_mailboxes() {
            write_smart_mailbox_toml(&self.config_root, &mailbox)?;
        }

        let snapshot = load_snapshot_from_disk(&self.config_root)?;
        *self.snapshot.write().map_err(lock_error)? = snapshot;
        Ok(())
    }

    /// Reads and parses `app.toml` directly from disk (bypasses snapshot).
    /// Used at startup to access daemon-only settings.
    pub fn read_app_toml(&self) -> Result<AppToml, ConfigError> {
        read_app_toml(&self.config_root)
    }
}

/// @spec docs/L1-accounts#configrepository-trait
#[cfg(test)]
mod tests;
