use std::fs;
use std::path::PathBuf;

use mail_domain::{
    AccountDriver, AccountSettings, AccountTransportSettings, AppSettings, SecretRef,
};
use serde::Deserialize;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const APP_DIR_NAME: &str = "mail";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapConfig {
    #[serde(default)]
    pub daemon: BootstrapDaemonConfig,
    #[serde(default)]
    pub seed: BootstrapSeedConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapDaemonConfig {
    pub bind: Option<String>,
    pub cors_origin: Option<String>,
    pub poll_interval_seconds: Option<u64>,
    pub data_root: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapSeedConfig {
    pub app: Option<BootstrapAppSettings>,
    #[serde(default)]
    pub accounts: Vec<BootstrapAccountConfig>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapAppSettings {
    pub default_account_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapAccountConfig {
    pub id: String,
    pub name: String,
    pub driver: AccountDriver,
    pub enabled: Option<bool>,
    #[serde(default)]
    pub transport: BootstrapAccountTransportConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapAccountTransportConfig {
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub secret_ref: Option<SecretRef>,
}

#[derive(Clone, Debug)]
pub struct ResolvedDaemonConfig {
    pub bootstrap_path: PathBuf,
    pub bind_address: String,
    pub cors_origin: String,
    pub poll_interval_seconds: u64,
    pub data_root: PathBuf,
}

pub fn load_bootstrap_config() -> Result<(ResolvedDaemonConfig, BootstrapSeedConfig), String> {
    let bootstrap_path = std::env::var("MAIL_BOOTSTRAP_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_bootstrap_path());
    let config = if bootstrap_path.exists() {
        let contents = fs::read_to_string(&bootstrap_path)
            .map_err(|err| format!("failed to read bootstrap config: {err}"))?;
        toml::from_str::<BootstrapConfig>(&contents)
            .map_err(|err| format!("failed to parse bootstrap config: {err}"))?
    } else {
        BootstrapConfig::default()
    };

    let data_root = config
        .daemon
        .data_root
        .unwrap_or_else(default_data_root_path);
    let daemon = ResolvedDaemonConfig {
        bootstrap_path,
        bind_address: config
            .daemon
            .bind
            .unwrap_or_else(|| "127.0.0.1:3001".to_string()),
        cors_origin: config
            .daemon
            .cors_origin
            .unwrap_or_else(|| "http://localhost:5173".to_string()),
        poll_interval_seconds: config.daemon.poll_interval_seconds.unwrap_or(60),
        data_root,
    };
    Ok((daemon, config.seed))
}

pub fn seed_app_settings(seed: &BootstrapSeedConfig) -> AppSettings {
    AppSettings {
        default_account_id: seed
            .app
            .as_ref()
            .and_then(|settings| settings.default_account_id.as_deref())
            .map(Into::into),
    }
}

pub fn seed_accounts(seed: &BootstrapSeedConfig) -> Result<Vec<AccountSettings>, String> {
    seed.accounts
        .iter()
        .map(|account| {
            let now = now_iso8601()?;
            Ok(AccountSettings {
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
            })
        })
        .collect()
}

fn default_bootstrap_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
        .join("bootstrap.toml")
}

fn default_data_root_path() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
}

fn now_iso8601() -> Result<String, String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|err| err.to_string())
}
