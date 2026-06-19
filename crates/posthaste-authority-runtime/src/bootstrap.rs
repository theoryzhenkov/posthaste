use std::fs;
use std::path::Path;

use posthaste_config::{default_smart_mailboxes, validate_safe_config_id, TomlConfigRepository};
use posthaste_domain::{
    now_iso8601 as domain_now_iso8601, validate_snapshot, AccountDriver, AccountSettings,
    AccountTransportSettings, AppSettings, ConfigRepository, ConfigSnapshot, SecretRef,
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

    let app_settings = bootstrap.seed.app.as_ref().map(|app_seed| AppSettings {
        default_account_id: app_seed.default_account_id.as_deref().map(Into::into),
        automation_rules: Vec::new(),
        automation_drafts: Vec::new(),
        ..Default::default()
    });
    let sources = bootstrap_sources(&bootstrap.seed.accounts)?;
    let candidate = ConfigSnapshot {
        sources: sources.clone(),
        smart_mailboxes: default_smart_mailboxes(),
        app_settings: app_settings.clone().unwrap_or_default(),
    };
    validate_snapshot(&candidate).map_err(posthaste_domain::ConfigError::from)?;

    config_repo.initialize_defaults()?;
    for source in &sources {
        config_repo.save_source(source)?;
    }
    if let Some(settings) = &app_settings {
        config_repo.put_app_settings(settings)?;
    }

    Ok(())
}

fn bootstrap_sources(
    accounts: &[BootstrapAccountConfig],
) -> Result<Vec<AccountSettings>, AuthorityRuntimeBuildError> {
    accounts
        .iter()
        .map(|account| {
            validate_safe_config_id(&account.id)?;
            let now = domain_now_iso8601().map_err(AuthorityRuntimeBuildError::Clock)?;
            Ok(AccountSettings {
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
            })
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_root() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "posthaste-bootstrap-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            n
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_valid_bootstrap(path: &Path) {
        fs::write(
            path,
            r#"
[seed.app]
defaultAccountId = "primary"

[[seed.accounts]]
id = "primary"
name = "Primary"
driver = "mock"
"#,
        )
        .unwrap();
    }

    #[test]
    fn bootstrap_rejects_semantically_invalid_snapshot_before_importing() {
        let root = temp_root();
        let bootstrap_path = root.join("bootstrap.toml");
        fs::write(
            &bootstrap_path,
            r#"
[seed.app]
defaultAccountId = "missing"
"#,
        )
        .unwrap();
        let config_root = root.join("config");
        let repo = TomlConfigRepository::open(&config_root).unwrap();

        let error = initialize_config(&repo, Some(&bootstrap_path))
            .expect_err("dangling bootstrap default account should fail")
            .to_string();

        assert!(
            error.contains("default account 'missing' does not exist"),
            "error should mention dangling default account: {error}"
        );
        let snapshot = repo.load_snapshot().unwrap();
        assert!(snapshot.sources.is_empty());
        assert_eq!(snapshot.app_settings.default_account_id, None);
        assert!(
            !config_root.join("app.toml").exists(),
            "invalid bootstrap should leave an empty repo retryable"
        );

        write_valid_bootstrap(&bootstrap_path);
        initialize_config(&repo, Some(&bootstrap_path))
            .expect("corrected bootstrap should still import after initial failure");
        let snapshot = repo.load_snapshot().unwrap();
        assert_eq!(snapshot.sources.len(), 1);
        assert_eq!(
            snapshot
                .app_settings
                .default_account_id
                .as_ref()
                .map(|id| id.as_str()),
            Some("primary")
        );
    }

    #[test]
    fn bootstrap_rejects_unsafe_account_ids_before_importing_defaults() {
        let root = temp_root();
        let bootstrap_path = root.join("bootstrap.toml");
        fs::write(
            &bootstrap_path,
            r#"
[[seed.accounts]]
id = "bad/id"
name = "Bad"
driver = "mock"
"#,
        )
        .unwrap();
        let config_root = root.join("config");
        let repo = TomlConfigRepository::open(&config_root).unwrap();

        let error = initialize_config(&repo, Some(&bootstrap_path))
            .expect_err("unsafe bootstrap account id should fail before import")
            .to_string();

        assert!(
            error.contains("unsafe characters"),
            "error should mention unsafe id: {error}"
        );
        assert!(
            !config_root.join("app.toml").exists(),
            "unsafe bootstrap should leave an empty repo retryable"
        );

        write_valid_bootstrap(&bootstrap_path);
        initialize_config(&repo, Some(&bootstrap_path))
            .expect("corrected bootstrap should still import after unsafe id failure");
        assert_eq!(repo.load_snapshot().unwrap().sources.len(), 1);
    }
}
