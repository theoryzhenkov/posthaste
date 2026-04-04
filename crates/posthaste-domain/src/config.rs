use thiserror::Error;

use crate::{AccountId, AccountSettings, AppSettings, SmartMailbox, SmartMailboxId};

/// Full in-memory snapshot of all config: app settings, sources, smart mailboxes.
///
/// @spec docs/L1-accounts#configsnapshot
#[derive(Clone, Debug)]
pub struct ConfigSnapshot {
    pub app_settings: AppSettings,
    pub sources: Vec<AccountSettings>,
    pub smart_mailboxes: Vec<SmartMailbox>,
}

/// Delta returned by [`ConfigRepository::reload`]: added, changed, and removed sources.
///
/// @spec docs/L1-accounts#configdiff
#[derive(Clone, Debug)]
pub struct ConfigDiff {
    pub added_sources: Vec<AccountId>,
    pub changed_sources: Vec<AccountId>,
    pub removed_sources: Vec<AccountId>,
}

/// Errors from configuration persistence operations.
///
/// @spec docs/L1-accounts#configrepository-trait
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("parse error: {0}")]
    Parse(String),
}

/// Config persistence boundary for TOML-backed account and smart mailbox storage.
///
/// Implementations must be `Send + Sync` and support concurrent readers.
/// Reads serve from an in-memory snapshot after initialization; writes
/// use atomic write-fsync-rename.
///
/// @spec docs/L1-accounts#configrepository-trait
pub trait ConfigRepository: Send + Sync {
    /// Load a full in-memory snapshot of all config files.
    fn load_snapshot(&self) -> Result<ConfigSnapshot, ConfigError>;

    /// Re-read all config files from disk and return a diff against the cached snapshot.
    ///
    /// @spec docs/L1-accounts#configdiff
    fn reload(&self) -> Result<ConfigDiff, ConfigError>;

    /// Read global application settings.
    fn get_app_settings(&self) -> Result<AppSettings, ConfigError>;
    /// Persist global application settings.
    fn put_app_settings(&self, settings: &AppSettings) -> Result<(), ConfigError>;

    /// List all account configurations.
    fn list_sources(&self) -> Result<Vec<AccountSettings>, ConfigError>;
    /// Look up a single account by ID.
    fn get_source(&self, id: &AccountId) -> Result<Option<AccountSettings>, ConfigError>;
    /// Create or update an account configuration.
    fn save_source(&self, source: &AccountSettings) -> Result<(), ConfigError>;
    /// Delete an account configuration.
    fn delete_source(&self, id: &AccountId) -> Result<(), ConfigError>;

    /// List all smart mailbox configurations.
    fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError>;
    /// Look up a single smart mailbox by ID.
    fn get_smart_mailbox(&self, id: &SmartMailboxId) -> Result<Option<SmartMailbox>, ConfigError>;
    /// Create or update a smart mailbox configuration.
    fn save_smart_mailbox(&self, mailbox: &SmartMailbox) -> Result<(), ConfigError>;
    /// Delete a smart mailbox configuration.
    fn delete_smart_mailbox(&self, id: &SmartMailboxId) -> Result<(), ConfigError>;

    /// Restore all default smart mailboxes without deleting user-created ones.
    ///
    /// @spec docs/L1-accounts#smart-mailbox-defaults
    fn reset_default_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError>;
}

/// Thread-safe handle to a [`ConfigRepository`] implementation.
pub type SharedConfigRepository = std::sync::Arc<dyn ConfigRepository>;
