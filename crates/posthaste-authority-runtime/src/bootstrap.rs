use std::fs;
use std::path::Path;

use posthaste_config::TomlConfigRepository;
use posthaste_domain::{
    now_iso8601 as domain_now_iso8601, AccountDriver, AccountSettings, AccountTransportSettings,
    AppSettings, ConfigRepository, SecretRef,
};
use serde::Deserialize;

use crate::build::AuthorityRuntimeBuildError;

/// Initialize an empty config repository from a bootstrap file, or from default
/// config and smart mailboxes when no bootstrap is supplied.
pub(crate) fn initialize_config(
    config_repo: &TomlConfigRepository,
    bootstrap_path: Option<&Path>,
) -> Result<(), AuthorityRuntimeBuildError> {
    if !config_repo.is_empty() {
        return Ok(());
    }

    if let Some(bootstrap_path) = bootstrap_path {
        import_bootstrap(bootstrap_path, config_repo)
    } else {
        config_repo.initialize_defaults()?;
        Ok(())
    }
}

fn import_bootstrap(
    bootstrap_path: &Path,
    config_repo: &TomlConfigRepository,
) -> Result<(), AuthorityRuntimeBuildError> {
    let contents = fs::read_to_string(bootstrap_path).map_err(|err| {
        AuthorityRuntimeBuildError::BootstrapRead {
            path: bootstrap_path.to_path_buf(),
            source: err,
        }
    })?;
    let bootstrap: BootstrapConfig =
        toml::from_str(&contents).map_err(|err| AuthorityRuntimeBuildError::BootstrapParse {
            path: bootstrap_path.to_path_buf(),
            message: err.to_string(),
        })?;

    config_repo.initialize_defaults()?;

    if let Some(app_seed) = &bootstrap.seed.app {
        let settings = AppSettings {
            default_account_id: app_seed.default_account_id.as_deref().map(Into::into),
            automation_rules: Vec::new(),
            automation_drafts: Vec::new(),
            ..Default::default()
        };
        config_repo.put_app_settings(&settings)?;
    }

    for account in &bootstrap.seed.accounts {
        let now = domain_now_iso8601().map_err(AuthorityRuntimeBuildError::Clock)?;
        let source = AccountSettings {
            id: account.id.clone().into(),
            name: account.name.clone(),
            full_name: account.full_name.clone(),
            email_patterns: account.email_patterns.clone(),
            driver: account.driver.clone(),
            enabled: account.enabled.unwrap_or(true),
            appearance: None,
            transport: AccountTransportSettings {
                provider: account.transport.provider.clone(),
                auth: account.transport.auth.clone(),
                base_url: account.transport.base_url.clone(),
                username: account.transport.username.clone(),
                secret_ref: account.transport.secret_ref.clone(),
                imap: account.transport.imap.clone(),
                smtp: account.transport.smtp.clone(),
            },
            created_at: now.clone(),
            updated_at: now,
        };
        config_repo.save_source(&source)?;
    }

    Ok(())
}

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
    full_name: Option<String>,
    #[serde(default)]
    email_patterns: Vec<String>,
    driver: AccountDriver,
    enabled: Option<bool>,
    #[serde(default)]
    transport: BootstrapAccountTransportConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapAccountTransportConfig {
    #[serde(default)]
    provider: posthaste_domain::ProviderHint,
    #[serde(default)]
    auth: posthaste_domain::ProviderAuthKind,
    base_url: Option<String>,
    username: Option<String>,
    secret_ref: Option<SecretRef>,
    imap: Option<posthaste_domain::ImapTransportSettings>,
    smtp: Option<posthaste_domain::SmtpTransportSettings>,
}
