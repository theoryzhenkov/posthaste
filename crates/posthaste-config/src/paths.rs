//! Canonical filesystem roots. Every Posthaste process — the daemon, the
//! desktop shell, the client backend — resolves its config root (TOML
//! repository) and state root (SQLite store + bodies) through this module,
//! so all of them open the same directories.
//!
//! The layout is an install-continuity contract: existing installs already
//! have their data under these directories, so [`APP_DIR_NAME`] and the
//! environment override names are frozen.

use std::path::PathBuf;

/// Application directory name under the XDG base directories.
pub const APP_DIR_NAME: &str = "posthaste";

/// Environment variable overriding the config root.
pub const CONFIG_ROOT_ENV: &str = "POSTHASTE_CONFIG_ROOT";

/// Environment variable overriding the state root.
pub const STATE_ROOT_ENV: &str = "POSTHASTE_STATE_ROOT";

/// Config root: `$POSTHASTE_CONFIG_ROOT`, else [`default_config_root`].
pub fn config_root() -> PathBuf {
    std::env::var(CONFIG_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_config_root())
}

/// State root: `$POSTHASTE_STATE_ROOT`, else [`default_state_root`].
pub fn state_root() -> PathBuf {
    std::env::var(STATE_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_state_root())
}

/// Default config root: `$XDG_CONFIG_HOME/posthaste` or `~/.config/posthaste`.
pub fn default_config_root() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config").join(APP_DIR_NAME)
}

/// Default state root: `$XDG_DATA_HOME/posthaste` or `~/.local/share/posthaste`.
pub fn default_state_root() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share").join(APP_DIR_NAME)
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
