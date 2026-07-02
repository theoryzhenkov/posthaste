//! TOML-backed configuration persistence for accounts and smart mailboxes.
//!
//! @spec docs/L1-accounts#config-directory-layout

mod atomic;
pub mod daemon;
mod defaults;
mod repository;
mod schema;

pub use daemon::{load_daemon_settings, read_daemon_settings, DaemonSettings, TlsConfig};
pub use defaults::default_smart_mailboxes;
pub use repository::{validate_config_root, validate_safe_config_id, TomlConfigRepository};
