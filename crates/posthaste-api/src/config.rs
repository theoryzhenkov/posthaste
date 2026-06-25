use std::fs;
use std::path::{Path, PathBuf};

use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    now_iso8601 as domain_now_iso8601, AccountDriver, AccountSettings, AccountTransportSettings,
    AppSettings, ConfigError, ConfigRepository, SecretRef,
};
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

/// Runtime settings for the daemon process, read from `app.toml` `[daemon]`
/// section with environment variable overrides.
///
/// @spec docs/L1-accounts#apptoml
#[derive(Clone, Debug)]
pub struct DaemonSettings {
    pub bind_address: String,
    pub cors_origin: String,
    pub poll_interval_seconds: u64,
    pub log_level: String,
    /// When `true`, the `/v1` API enforces bearer-token + Origin/Host auth.
    /// Defaults to `true` (perimeter on); an explicit `app.toml`/env override
    /// can disable it.
    ///
    /// @spec docs/eph/DESIGN-L1-trust-model
    pub require_auth: bool,
    /// Backend role: when `true`, mount the runtime↔backend `link_router` so a
    /// remote runtime can drive this backend over HTTP. Default `false` (the
    /// bundled single-process deployment never exposes the link).
    ///
    /// @spec docs/replication/L1#10-deployment-topology
    pub link_serve: bool,
    /// Shared bearer token for the link surface — required from connecting
    /// runtimes when serving, and presented to the remote backend when
    /// connecting. Serving without a token (under `require_auth`) is refused.
    pub link_token: Option<String>,
    /// Runtime role: when set, this process connects to a remote backend at this
    /// base URL over the link instead of using the in-process one.
    pub link_backend_url: Option<String>,
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

/// Read daemon settings from `app.toml` `[daemon]` section, with env var
/// overrides for bind address, CORS origin, and poll interval.
///
/// @spec docs/L1-accounts#apptoml
/// Open the config repository at `config_root` and read the daemon settings.
/// A convenience for hosts (the lean runtime daemon) that only depend on
/// `posthaste-api` and so cannot construct a `TomlConfigRepository` themselves.
pub fn load_daemon_settings(config_root: &Path) -> DaemonSettings {
    let repo = TomlConfigRepository::open(config_root).expect("failed to open config directory");
    read_daemon_settings(&repo).expect("failed to read runtime settings")
}

pub fn read_daemon_settings(
    config_repo: &TomlConfigRepository,
) -> Result<DaemonSettings, ConfigError> {
    let app_toml = config_repo.read_app_toml()?;

    // Also check env vars that may override
    let bind = std::env::var("POSTHASTE_BIND")
        .ok()
        .or(app_toml.daemon.bind)
        .unwrap_or_else(|| "127.0.0.1:3001".to_string());

    let cors_origin = std::env::var("POSTHASTE_CORS_ORIGIN")
        .ok()
        .or(app_toml.daemon.cors_origin)
        .unwrap_or_else(|| "http://localhost:5173".to_string());

    let poll_interval_seconds = std::env::var("POSTHASTE_POLL_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .or(app_toml.daemon.poll_interval_seconds)
        .unwrap_or(60);

    let log_level = std::env::var("POSTHASTE_LOG_LEVEL")
        .ok()
        .or(app_toml.logging.level)
        .unwrap_or_else(|| "info".to_string());

    // Perimeter auth defaults ON. Explicit config/env wins (an app.toml
    // `[daemon] require_auth` or `POSTHASTE_REQUIRE_AUTH=false` still disables
    // it); absence resolves to enabled.
    //
    // @spec docs/eph/DESIGN-L1-trust-model
    let require_auth = std::env::var("POSTHASTE_REQUIRE_AUTH")
        .ok()
        .and_then(|v| parse_bool_flag(&v))
        .or(app_toml.daemon.require_auth)
        .unwrap_or(true);

    // Runtime↔backend link roles (default: in-process, not served). Env wins
    // over `[link]` so a split can be dogfooded without editing config.
    let link_serve = std::env::var("POSTHASTE_LINK_SERVE")
        .ok()
        .and_then(|v| parse_bool_flag(&v))
        .or(app_toml.link.serve)
        .unwrap_or(false);
    let link_token = std::env::var("POSTHASTE_LINK_TOKEN")
        .ok()
        .filter(|token| !token.is_empty())
        .or(app_toml.link.token);
    let link_backend_url = std::env::var("POSTHASTE_LINK_BACKEND_URL")
        .ok()
        .filter(|url| !url.is_empty())
        .or(app_toml.link.backend_url);

    Ok(DaemonSettings {
        bind_address: bind,
        cors_origin,
        poll_interval_seconds,
        log_level,
        require_auth,
        link_serve,
        link_token,
        link_backend_url,
    })
}

mod bootstrap;
pub use bootstrap::import_bootstrap;

// -- Helpers --

fn parse_bool_flag(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}


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
