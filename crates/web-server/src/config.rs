use std::fs;
use std::path::{Path, PathBuf};

use mail_config::TomlConfigRepository;
use mail_domain::{
    now_iso8601 as domain_now_iso8601, AccountDriver, AccountSettings, AccountTransportSettings,
    AppSettings, ConfigError, ConfigRepository, SecretRef,
};
use serde::Deserialize;

/// Application directory name used under XDG paths.
const APP_DIR_NAME: &str = "mail";

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
}

/// Resolve config, state, and bootstrap paths from environment variables
/// or XDG defaults.
///
/// @spec docs/L1-accounts#config-directory-layout
pub fn resolve_roots() -> ResolvedRoots {
    let config_root = std::env::var("MAIL_CONFIG_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_config_root());

    let state_root = std::env::var("MAIL_STATE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_state_root());

    let bootstrap_path = std::env::var("MAIL_BOOTSTRAP_PATH")
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
pub fn read_daemon_settings(
    config_repo: &TomlConfigRepository,
) -> Result<DaemonSettings, ConfigError> {
    let app_toml = config_repo.read_app_toml()?;

    // Also check env vars that may override
    let bind = std::env::var("MAIL_BIND")
        .ok()
        .or(app_toml.daemon.bind)
        .unwrap_or_else(|| "127.0.0.1:3001".to_string());

    let cors_origin = std::env::var("MAIL_CORS_ORIGIN")
        .ok()
        .or(app_toml.daemon.cors_origin)
        .unwrap_or_else(|| "http://localhost:5173".to_string());

    let poll_interval_seconds = std::env::var("MAIL_POLL_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .or(app_toml.daemon.poll_interval_seconds)
        .unwrap_or(60);

    let log_level = std::env::var("MAIL_LOG_LEVEL")
        .ok()
        .or(app_toml.logging.level)
        .unwrap_or_else(|| "info".to_string());

    Ok(DaemonSettings {
        bind_address: bind,
        cors_origin,
        poll_interval_seconds,
        log_level,
    })
}

/// Import a bootstrap TOML file: initialize defaults, then apply seed
/// app settings and account definitions.
///
/// @spec docs/L1-accounts#initialization
pub fn import_bootstrap(
    bootstrap_path: &Path,
    config_repo: &TomlConfigRepository,
) -> Result<(), String> {
    let contents = fs::read_to_string(bootstrap_path)
        .map_err(|err| format!("failed to read bootstrap config: {err}"))?;
    let bootstrap: BootstrapConfig = toml::from_str(&contents)
        .map_err(|err| format!("failed to parse bootstrap config: {err}"))?;

    // Initialize defaults first (creates app.toml + default smart mailboxes)
    config_repo
        .initialize_defaults()
        .map_err(|err| format!("failed to initialize defaults: {err}"))?;

    // Apply seed settings
    if let Some(app_seed) = &bootstrap.seed.app {
        let settings = AppSettings {
            default_account_id: app_seed.default_account_id.as_deref().map(Into::into),
        };
        config_repo
            .put_app_settings(&settings)
            .map_err(|err| format!("failed to write app settings: {err}"))?;
    }

    // Import seed accounts
    for account in &bootstrap.seed.accounts {
        let now = domain_now_iso8601()?;
        let source = AccountSettings {
            id: account.id.clone().into(),
            name: account.name.clone(),
            driver: account.driver.clone(),
            enabled: account.enabled.unwrap_or(true),
            transport: AccountTransportSettings {
                base_url: account.transport.base_url.clone(),
                username: account.transport.username.clone(),
                secret_ref: account.transport.secret_ref.clone(),
            },
            created_at: now.clone(),
            updated_at: now,
        };
        config_repo
            .save_source(&source)
            .map_err(|err| format!("failed to write source '{}': {err}", account.id))?;
    }

    Ok(())
}

// -- Bootstrap TOML types (for import only) --

/// Top-level bootstrap config file structure.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapConfig {
    #[serde(default)]
    seed: BootstrapSeedConfig,
}

/// Seed data section: app settings and initial accounts.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapSeedConfig {
    app: Option<BootstrapAppSettings>,
    #[serde(default)]
    accounts: Vec<BootstrapAccountConfig>,
}

/// Bootstrap app-level overrides.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapAppSettings {
    default_account_id: Option<String>,
}

/// A seed account definition in the bootstrap file.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapAccountConfig {
    id: String,
    name: String,
    driver: AccountDriver,
    enabled: Option<bool>,
    #[serde(default)]
    transport: BootstrapAccountTransportConfig,
}

/// Transport section of a seed account.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapAccountTransportConfig {
    base_url: Option<String>,
    username: Option<String>,
    secret_ref: Option<SecretRef>,
}

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
