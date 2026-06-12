use std::fs;
use std::path::Path;

use posthaste_domain::{
    now_iso8601 as domain_now_iso8601, AccountSettings, ConfigError, ConfigSnapshot, SmartMailbox,
    RFC3339_EPOCH,
};

use crate::atomic::atomic_write;
use crate::schema::{AppToml, SmartMailboxToml, SourceToml};

pub(super) fn load_snapshot_from_disk(config_root: &Path) -> Result<ConfigSnapshot, ConfigError> {
    let app_settings = read_app_toml(config_root)?
        .to_app_settings()
        .map_err(ConfigError::Parse)?;

    let sources = load_sources(config_root)?;
    let smart_mailboxes = load_smart_mailboxes(config_root)?;

    Ok(ConfigSnapshot {
        app_settings,
        sources,
        smart_mailboxes,
    })
}

/// Reads and parses `app.toml`, returning defaults if the file does not exist.
pub(super) fn read_app_toml(config_root: &Path) -> Result<AppToml, ConfigError> {
    let path = config_root.join("app.toml");
    if !path.exists() {
        return Ok(AppToml::default());
    }
    let content = fs::read_to_string(&path).map_err(io_error)?;
    toml::from_str(&content).map_err(|e| ConfigError::Parse(format!("app.toml: {e}")))
}

/// Serializes and atomically writes `app.toml`.
pub(super) fn write_app_toml(config_root: &Path, app: &AppToml) -> Result<(), ConfigError> {
    let content = toml::to_string_pretty(app).map_err(|e| ConfigError::Parse(e.to_string()))?;
    atomic_write(&config_root.join("app.toml"), content.as_bytes())
}

/// Reads all `sources/*.toml` files, validates filename-ID match, and returns
/// sorted account settings.
///
/// @spec docs/L1-accounts#config-directory-layout
pub(super) fn load_sources(config_root: &Path) -> Result<Vec<AccountSettings>, ConfigError> {
    let dir = config_root.join("sources");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut sources = Vec::new();
    for entry in fs::read_dir(&dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        if path.extension().map(|e| e == "toml").unwrap_or(false) {
            let content = fs::read_to_string(&path).map_err(io_error)?;
            let source: SourceToml = toml::from_str(&content)
                .map_err(|e| ConfigError::Parse(format!("{}: {e}", path.display())))?;

            validate_filename_matches_id(&path, &source.id)?;
            sources.push(source.to_account_settings().map_err(|e| {
                ConfigError::Parse(format!("{}: invalid automation rule: {e}", path.display()))
            })?);
        }
    }
    sources.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(sources)
}

/// Reads all `smart-mailboxes/*.toml` files, validates filename-ID match, and
/// returns mailboxes sorted by position then name.
///
/// @spec docs/L1-accounts#config-directory-layout
pub(super) fn load_smart_mailboxes(config_root: &Path) -> Result<Vec<SmartMailbox>, ConfigError> {
    let dir = config_root.join("smart-mailboxes");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut mailboxes = Vec::new();
    for entry in fs::read_dir(&dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        if path.extension().map(|e| e == "toml").unwrap_or(false) {
            let content = fs::read_to_string(&path).map_err(io_error)?;
            let toml_mailbox: SmartMailboxToml = toml::from_str(&content)
                .map_err(|e| ConfigError::Parse(format!("{}: {e}", path.display())))?;

            validate_filename_matches_id(&path, &toml_mailbox.id)?;
            let mailbox = toml_mailbox
                .to_smart_mailbox()
                .map_err(|e| ConfigError::Parse(format!("{}: {e}", path.display())))?;
            mailboxes.push(mailbox);
        }
    }
    mailboxes.sort_by(|a, b| a.position.cmp(&b.position).then(a.name.cmp(&b.name)));
    Ok(mailboxes)
}

/// Serializes and atomically writes a smart mailbox TOML file.
pub(super) fn write_smart_mailbox_toml(
    config_root: &Path,
    mailbox: &SmartMailbox,
) -> Result<(), ConfigError> {
    let toml_struct = SmartMailboxToml::from_smart_mailbox(mailbox);
    let content =
        toml::to_string_pretty(&toml_struct).map_err(|e| ConfigError::Parse(e.to_string()))?;
    let path = config_root
        .join("smart-mailboxes")
        .join(format!("{}.toml", mailbox.id));
    atomic_write(&path, content.as_bytes())
}

/// Rejects IDs containing path separators, parent traversal, or null bytes to
/// prevent path injection.
///
/// @spec docs/L1-accounts#id-validation
pub(super) fn validate_safe_id(id: &str) -> Result<(), ConfigError> {
    if id.is_empty()
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || id.contains('\0')
    {
        return Err(ConfigError::Validation(format!(
            "id '{id}' contains unsafe characters"
        )));
    }
    Ok(())
}

/// Enforces that the TOML filename stem matches the `id` field inside the file.
///
/// @spec docs/L1-accounts#assertions
pub(super) fn validate_filename_matches_id(path: &Path, id: &str) -> Result<(), ConfigError> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if stem != id {
        return Err(ConfigError::Validation(format!(
            "filename '{}' does not match id '{id}' in {}",
            stem,
            path.display()
        )));
    }
    Ok(())
}

/// Returns the current time as an ISO 8601 string, falling back to epoch.
pub(super) fn now_iso8601() -> String {
    domain_now_iso8601().unwrap_or_else(|_| RFC3339_EPOCH.to_string())
}

/// Wraps an I/O error into `ConfigError::Io`.
pub(super) fn io_error(err: std::io::Error) -> ConfigError {
    ConfigError::Io(err.to_string())
}

/// Wraps a lock-poisoned error into `ConfigError::Io`.
pub(super) fn lock_error<T>(_: T) -> ConfigError {
    ConfigError::Io("config lock poisoned".to_string())
}
