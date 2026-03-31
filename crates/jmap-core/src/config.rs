use thiserror::Error;

use crate::{AccountId, AccountSettings, AppSettings, SmartMailbox, SmartMailboxId};

#[derive(Clone, Debug)]
pub struct ConfigSnapshot {
    pub app_settings: AppSettings,
    pub sources: Vec<AccountSettings>,
    pub smart_mailboxes: Vec<SmartMailbox>,
}

#[derive(Clone, Debug)]
pub struct ConfigDiff {
    pub added_sources: Vec<AccountId>,
    pub changed_sources: Vec<AccountId>,
    pub removed_sources: Vec<AccountId>,
}

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

pub trait ConfigRepository: Send + Sync {
    fn load_snapshot(&self) -> Result<ConfigSnapshot, ConfigError>;
    fn reload(&self) -> Result<ConfigDiff, ConfigError>;

    fn get_app_settings(&self) -> Result<AppSettings, ConfigError>;
    fn put_app_settings(&self, settings: &AppSettings) -> Result<(), ConfigError>;

    fn list_sources(&self) -> Result<Vec<AccountSettings>, ConfigError>;
    fn get_source(&self, id: &AccountId) -> Result<Option<AccountSettings>, ConfigError>;
    fn save_source(&self, source: &AccountSettings) -> Result<(), ConfigError>;
    fn delete_source(&self, id: &AccountId) -> Result<(), ConfigError>;

    fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError>;
    fn get_smart_mailbox(&self, id: &SmartMailboxId) -> Result<Option<SmartMailbox>, ConfigError>;
    fn save_smart_mailbox(&self, mailbox: &SmartMailbox) -> Result<(), ConfigError>;
    fn delete_smart_mailbox(&self, id: &SmartMailboxId) -> Result<(), ConfigError>;
    fn reset_default_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError>;
}

pub type SharedConfigRepository = std::sync::Arc<dyn ConfigRepository>;
