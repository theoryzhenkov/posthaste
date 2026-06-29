use std::fs;
use std::path::Path;

use posthaste_domain::{
    now_iso8601 as domain_now_iso8601, validate_snapshot, AccountSettings, ConfigError,
    ConfigSnapshot, SmartMailbox, RFC3339_EPOCH,
};

use crate::atomic::atomic_write;
use crate::schema::{AppToml, SmartMailboxToml, SourceToml};

pub(super) fn load_snapshot_from_disk(config_root: &Path) -> Result<ConfigSnapshot, ConfigError> {
    let app_settings = read_app_toml(config_root)?
        .to_app_settings()
        .map_err(ConfigError::Parse)?;

    let sources = load_sources(config_root)?;
    let smart_mailboxes = load_smart_mailboxes(config_root)?;

    let snapshot = ConfigSnapshot {
        app_settings,
        sources,
        smart_mailboxes,
    };
    validate_snapshot(&snapshot).map_err(ConfigError::from)?;
    Ok(snapshot)
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

/// The top-level keys `AppToml` owns. The lossless write overwrites exactly
/// these from the struct and leaves everything else in the file untouched
/// (comments, ordering, and any unknown section a user or LLM added). Add a key
/// here whenever `AppToml` gains a top-level section — forgetting only means the
/// new section is not written (a visible bug), never that another section is
/// silently dropped (the round-trip footgun this fixes).
const APP_TOML_MANAGED_KEYS: &[&str] = &[
    "schema_version",
    "default_source_id",
    "automations",
    "draft_automations",
    "daemon",
    "logging",
    "cache",
    "appearance",
    "notifications",
    "mailbox_colors",
    "link",
];

/// Atomically write `value`'s managed top-level keys into the existing file at
/// `path` without disturbing anything else: comments, key ordering, and unknown
/// sections survive. A managed key absent from `value` (a cleared `Option`) is
/// removed. This is the lossless replacement for `to_string_pretty`, which
/// rebuilt the whole file from the struct and dropped everything the struct does
/// not model.
fn write_managed_toml<T: serde::Serialize>(
    path: &Path,
    value: &T,
    managed_keys: &[&str],
) -> Result<(), ConfigError> {
    let existing = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(io_error(err)),
    };
    let mut doc = existing
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| ConfigError::Parse(format!("{}: {e}", path.display())))?;
    let managed = toml::to_string_pretty(value)
        .map_err(|e| ConfigError::Parse(e.to_string()))?
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| ConfigError::Parse(e.to_string()))?;
    for key in managed_keys {
        match managed.get(key) {
            Some(item) => doc[*key] = item.clone(),
            None => {
                doc.remove(key);
            }
        }
    }
    atomic_write(path, doc.to_string().as_bytes())
}

/// Atomically writes `app.toml`, preserving comments + unknown sections.
pub(super) fn write_app_toml(config_root: &Path, app: &AppToml) -> Result<(), ConfigError> {
    write_managed_toml(&config_root.join("app.toml"), app, APP_TOML_MANAGED_KEYS)
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

/// Serializes and atomically writes an account source TOML file.
/// The top-level keys `SourceToml` owns (snake_case = the field names; no
/// `rename_all`). Add a key here when `SourceToml` gains a top-level field.
const SOURCE_TOML_MANAGED_KEYS: &[&str] = &[
    "id",
    "name",
    "full_name",
    "signature",
    "email_patterns",
    "driver",
    "enabled",
    "appearance",
    "transport",
    "created_at",
    "updated_at",
];

pub(super) fn write_source_toml(
    config_root: &Path,
    source: &AccountSettings,
) -> Result<(), ConfigError> {
    let source_toml = SourceToml::from_account_settings(source);
    let path = config_root
        .join("sources")
        .join(format!("{}.toml", source.id));
    write_managed_toml(&path, &source_toml, SOURCE_TOML_MANAGED_KEYS)
}

/// Serializes and atomically writes a smart mailbox TOML file.
/// The top-level keys `SmartMailboxToml` owns (snake_case; no `rename_all`).
const SMART_MAILBOX_TOML_MANAGED_KEYS: &[&str] = &[
    "id",
    "name",
    "position",
    "kind",
    "default_key",
    "role",
    "parent_id",
    "rule",
    "created_at",
    "updated_at",
];

pub(super) fn write_smart_mailbox_toml(
    config_root: &Path,
    mailbox: &SmartMailbox,
) -> Result<(), ConfigError> {
    let toml_struct = SmartMailboxToml::from_smart_mailbox(mailbox);
    let path = config_root
        .join("smart-mailboxes")
        .join(format!("{}.toml", mailbox.id));
    write_managed_toml(&path, &toml_struct, SMART_MAILBOX_TOML_MANAGED_KEYS)
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
    validate_safe_id(id)?;
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

#[cfg(test)]
mod lossless_write_tests {
    use super::*;

    #[test]
    fn write_app_toml_preserves_comments_and_unknown_sections() {
        let dir = std::env::temp_dir().join(format!(
            "ph-cfg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.toml");
        fs::write(
            &path,
            "# my notes\nschema_version = 1\ndefault_source_id = \"acct-a\"\n\n# keep this too\n[custom]\nmy_key = \"keep me\"\n",
        )
        .unwrap();

        // Load (the struct can't model [custom]), change a managed field, write back.
        let mut app = read_app_toml(&dir).unwrap();
        app.default_source_id = Some("acct-b".to_string());
        write_app_toml(&dir, &app).unwrap();

        let after = fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("# my notes"),
            "leading comment dropped:\n{after}"
        );
        assert!(
            after.contains("[custom]") && after.contains("keep me"),
            "unknown section dropped:\n{after}"
        );
        assert!(
            after.contains("# keep this too"),
            "section comment dropped:\n{after}"
        );
        assert!(
            after.contains("acct-b"),
            "managed field not updated:\n{after}"
        );
        assert!(
            !after.contains("acct-a"),
            "stale managed value lingered:\n{after}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_app_toml_preserves_appearance_and_glass_blooms() {
        let dir = std::env::temp_dir().join(format!(
            "ph-cfg-appearance-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.toml");
        std::fs::write(
            &path,
            r#"# note
[appearance]
mode = "dark"
palette_preset = "glass"
density = "compact"
accent_hue = 250
[[appearance.glass_theme.blooms]]
id = "bloom-1"
hue = 285
x = 20
y = 10
opacity = 0.35
radius = 45
"#,
        )
        .unwrap();

        // Parse: [appearance] + the glass blooms array deserialize.
        let app = read_app_toml(&dir).unwrap();
        assert!(app.appearance.is_some(), "appearance did not parse");
        let glass = app.appearance.as_ref().unwrap().glass_theme.as_ref();
        assert!(
            glass.is_some() && glass.unwrap().blooms.len() == 1,
            "glass bloom did not parse"
        );

        // Back-compat: legacy `palette_preset` reads into `theme`, and the legacy
        // top-level `accent_hue` seeds both per-mode color tables.
        let settings = app.to_app_settings().unwrap();
        let appearance = settings.appearance.as_ref().unwrap();
        assert_eq!(appearance.theme.as_deref(), Some("glass"));
        assert_eq!(
            appearance.light.as_ref().and_then(|c| c.accent_hue),
            Some(250)
        );
        assert_eq!(
            appearance.dark.as_ref().and_then(|c| c.accent_hue),
            Some(250)
        );

        // Round-trip: AppSettings -> AppToml -> lossless write.
        let toml_struct = AppToml::from_app_settings(&settings, &app);
        write_app_toml(&dir, &toml_struct).unwrap();

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("[appearance]"),
            "appearance section dropped:\n{after}"
        );
        assert!(after.contains("glass"), "palette value dropped:\n{after}");
        // The legacy keys are migrated away on write (per-mode shape replaces them).
        assert!(
            after.contains("theme = \"glass\"") && !after.contains("palette_preset"),
            "theme not migrated:\n{after}"
        );
        assert!(
            after.contains("[appearance.light]") && after.contains("[appearance.dark]"),
            "per-mode color tables not written:\n{after}"
        );
        assert!(
            after.contains("[[appearance.glass_theme.blooms]]"),
            "blooms array dropped:\n{after}"
        );
        assert!(after.contains("bloom-1"), "bloom id dropped:\n{after}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_app_toml_round_trips_mailbox_colors() {
        let dir = std::env::temp_dir().join(format!(
            "ph-cfg-mboxcolor-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.toml");
        std::fs::write(
            &path,
            r#"[[mailbox_colors]]
source_id = "primary"
mailbox_id = "INBOX"
hue = 200

[[mailbox_colors]]
source_id = "primary"
mailbox_id = "Receipts"
hue = 45

[custom]
k = 1
"#,
        )
        .unwrap();

        let app = read_app_toml(&dir).unwrap();
        let settings = app.to_app_settings().unwrap();
        assert_eq!(settings.mailbox_colors.len(), 2);
        assert_eq!(settings.mailbox_colors[0].mailbox_id.as_str(), "INBOX");
        assert_eq!(settings.mailbox_colors[0].hue, 200);

        // Lossless round-trip: AppSettings -> AppToml -> write.
        let toml_struct = AppToml::from_app_settings(&settings, &app);
        write_app_toml(&dir, &toml_struct).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(
            after.contains("[[mailbox_colors]]") && after.contains("Receipts"),
            "mailbox_colors dropped:\n{after}"
        );
        // The unmanaged section survives the lossless write.
        assert!(
            after.contains("[custom]"),
            "unknown section dropped:\n{after}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_app_toml_removes_a_cleared_managed_key() {
        let dir = std::env::temp_dir().join(format!(
            "ph-cfg-clear-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.toml");
        fs::write(&path, "default_source_id = \"acct-a\"\n[custom]\nk = 1\n").unwrap();

        let mut app = read_app_toml(&dir).unwrap();
        app.default_source_id = None; // cleared in the UI
        write_app_toml(&dir, &app).unwrap();

        let after = fs::read_to_string(&path).unwrap();
        assert!(
            !after.contains("default_source_id"),
            "cleared key not removed:\n{after}"
        );
        assert!(
            after.contains("[custom]"),
            "unknown section dropped on clear:\n{after}"
        );

        fs::remove_dir_all(&dir).ok();
    }
}
