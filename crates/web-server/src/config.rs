use std::fs;
use std::path::{Path, PathBuf};

use mail_config::TomlConfigRepository;
use mail_domain::{
    AccountDriver, AccountSettings, AccountTransportSettings, AppSettings, ConfigRepository,
    SecretRef,
};
use serde::Deserialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const APP_DIR_NAME: &str = "mail";

#[derive(Clone, Debug)]
pub struct ResolvedRoots {
    pub config_root: PathBuf,
    pub state_root: PathBuf,
    pub bootstrap_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct DaemonSettings {
    pub bind_address: String,
    pub cors_origin: String,
    pub poll_interval_seconds: u64,
}

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

pub fn read_daemon_settings(
    config_repo: &TomlConfigRepository,
    _roots: &ResolvedRoots,
) -> DaemonSettings {
    let app_toml = config_repo.read_app_toml().unwrap_or_default();

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

    DaemonSettings {
        bind_address: bind,
        cors_origin,
        poll_interval_seconds,
    }
}

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
        let now = now_iso8601()?;
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

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapConfig {
    #[serde(default)]
    seed: BootstrapSeedConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapSeedConfig {
    app: Option<BootstrapAppSettings>,
    #[serde(default)]
    accounts: Vec<BootstrapAccountConfig>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapAppSettings {
    default_account_id: Option<String>,
}

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

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapAccountTransportConfig {
    base_url: Option<String>,
    username: Option<String>,
    secret_ref: Option<SecretRef>,
}

// -- Helpers --

fn default_config_root() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
}

fn default_state_root() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
}

fn default_bootstrap_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
        .join("bootstrap.toml")
}

fn now_iso8601() -> Result<String, String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|err| err.to_string())
}
