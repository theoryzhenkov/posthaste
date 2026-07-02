use super::*;

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
            automation_rules: Vec::new(),
            automation_drafts: Vec::new(),
            ..Default::default()
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
            full_name: account.full_name.clone(),
            signature: None,
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
        config_repo
            .save_source(&source)
            .map_err(|err| format!("failed to write source '{}': {err}", account.id))?;
    }

    Ok(())
}

// Parse a boolean env-var flag, accepting common truthy/falsy spellings.
// Returns `None` for unrecognized values so the config/default fallback wins.

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
    full_name: Option<String>,
    #[serde(default)]
    email_patterns: Vec<String>,
    driver: AccountDriver,
    enabled: Option<bool>,
    #[serde(default)]
    transport: BootstrapAccountTransportConfig,
}

/// Transport section of a seed account.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BootstrapAccountTransportConfig {
    #[serde(default)]
    provider: posthaste_domain_model::ProviderHint,
    #[serde(default)]
    auth: posthaste_domain_model::ProviderAuthKind,
    base_url: Option<String>,
    username: Option<String>,
    secret_ref: Option<SecretRef>,
    imap: Option<posthaste_domain_model::ImapTransportSettings>,
    smtp: Option<posthaste_domain_model::SmtpTransportSettings>,
}
