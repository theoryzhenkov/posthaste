use std::fs;
use std::path::{Path, PathBuf};

use posthaste_config::TomlConfigRepository;
use posthaste_domain_model::{
    now_iso8601 as domain_now_iso8601, AccountDriver, AccountSettings, AccountTransportSettings,
    AppSettings, SecretRef,
};
use posthaste_domain_service::ConfigRepository;
use serde::Deserialize;

/// Application directory name used under XDG paths.
const APP_DIR_NAME: &str = "posthaste";

/// Resolved filesystem paths for config, state, and optional bootstrap template.
///
/// @spec docs/L1-accounts#config-directory-layout
#[derive(Clone, Debug)]
pub struct ResolvedRoots {
    pub config_root: PathBuf,
    pub state_root: PathBuf,
    pub bootstrap_path: Option<PathBuf>,
}

/// Resolve config, state, and bootstrap paths from environment variables
/// or XDG defaults.
///
/// @spec docs/L1-accounts#config-directory-layout
pub fn resolve_roots() -> ResolvedRoots {
    let config_root = std::env::var("POSTHASTE_CONFIG_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_config_root());

    let state_root = std::env::var("POSTHASTE_STATE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_state_root());

    let bootstrap_path = std::env::var("POSTHASTE_BOOTSTRAP_PATH")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            let default = default_bootstrap_path();
            if default.exists() {
                Some(default)
            } else {
                None
            }
        });

    ResolvedRoots {
        config_root,
        state_root,
        bootstrap_path,
    }
}

mod bootstrap;
pub use bootstrap::import_bootstrap;

// -- Helpers --

/// Default config root: `$XDG_CONFIG_HOME/mail` or `~/.config/mail`.
fn default_config_root() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join(APP_DIR_NAME)
}

/// Default state root: `$XDG_DATA_HOME/mail` or `~/.local/share/mail`.
fn default_state_root() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share").join(APP_DIR_NAME)
}

/// Resolve an XDG directory from an env var or fall back to `$HOME/{suffix}`.
fn xdg_dir(env_var: &str, fallback_suffix: &str) -> PathBuf {
    std::env::var(env_var)
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(fallback_suffix)
        })
}

/// Default bootstrap file location: `$XDG_CONFIG_HOME/mail/bootstrap.toml`.
fn default_bootstrap_path() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config")
        .join(APP_DIR_NAME)
        .join("bootstrap.toml")
}
