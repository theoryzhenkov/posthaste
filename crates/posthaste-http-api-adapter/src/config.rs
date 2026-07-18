use std::fs;
use std::path::{Path, PathBuf};

use posthaste_config::TomlConfigRepository;
use posthaste_domain_model::{
    now_iso8601 as domain_now_iso8601, AccountDriver, AccountSettings, AccountTransportSettings,
    AppSettings, SecretRef,
};
use posthaste_domain_service::ConfigRepository;
use serde::Deserialize;

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
/// or XDG defaults. Config and state roots come from the canonical shared
/// resolver in [`posthaste_config::paths`], so every embedding opens the
/// same directories.
///
/// @spec docs/L1-accounts#config-directory-layout
pub fn resolve_roots() -> ResolvedRoots {
    let config_root = posthaste_config::paths::config_root();
    let state_root = posthaste_config::paths::state_root();

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

/// Default bootstrap file location: `bootstrap.toml` under the default
/// config root (deliberately not the env-overridden root).
fn default_bootstrap_path() -> PathBuf {
    posthaste_config::paths::default_config_root().join("bootstrap.toml")
}
