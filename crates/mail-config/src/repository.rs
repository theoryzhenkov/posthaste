use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use mail_domain::{
    AccountId, AccountSettings, AppSettings, ConfigDiff, ConfigError, ConfigRepository,
    ConfigSnapshot, SmartMailbox, SmartMailboxId,
};

use crate::atomic::atomic_write;
use crate::defaults::default_smart_mailboxes;
use crate::schema::{AppToml, SmartMailboxToml, SourceToml};

pub struct TomlConfigRepository {
    config_root: PathBuf,
    snapshot: RwLock<ConfigSnapshot>,
}

impl TomlConfigRepository {
    pub fn open(config_root: impl Into<PathBuf>) -> Result<Self, ConfigError> {
        let config_root = config_root.into();
        fs::create_dir_all(&config_root).map_err(io_error)?;
        fs::create_dir_all(config_root.join("sources")).map_err(io_error)?;
        fs::create_dir_all(config_root.join("smart-mailboxes")).map_err(io_error)?;

        let snapshot = load_snapshot_from_disk(&config_root)?;
        Ok(Self {
            config_root,
            snapshot: RwLock::new(snapshot),
        })
    }

    pub fn config_root(&self) -> &Path {
        &self.config_root
    }

    pub fn is_empty(&self) -> bool {
        !self.config_root.join("app.toml").exists()
    }

    pub fn initialize_defaults(&self) -> Result<(), ConfigError> {
        let app = AppToml {
            schema_version: 1,
            default_source_id: None,
            daemon: Default::default(),
        };
        write_app_toml(&self.config_root, &app)?;

        for mailbox in default_smart_mailboxes() {
            write_smart_mailbox_toml(&self.config_root, &mailbox)?;
        }

        let snapshot = load_snapshot_from_disk(&self.config_root)?;
        *self.snapshot.write().map_err(lock_error)? = snapshot;
        Ok(())
    }

    pub fn read_app_toml(&self) -> Result<AppToml, ConfigError> {
        read_app_toml(&self.config_root)
    }
}

impl ConfigRepository for TomlConfigRepository {
    fn load_snapshot(&self) -> Result<ConfigSnapshot, ConfigError> {
        Ok(self.snapshot.read().map_err(lock_error)?.clone())
    }

    fn reload(&self) -> Result<ConfigDiff, ConfigError> {
        let old = self.snapshot.read().map_err(lock_error)?.clone();
        let new = load_snapshot_from_disk(&self.config_root)?;

        let old_source_ids: std::collections::HashSet<_> =
            old.sources.iter().map(|s| s.id.clone()).collect();
        let new_source_ids: std::collections::HashSet<_> =
            new.sources.iter().map(|s| s.id.clone()).collect();

        let added_sources = new_source_ids
            .difference(&old_source_ids)
            .cloned()
            .collect();
        let removed_sources = old_source_ids
            .difference(&new_source_ids)
            .cloned()
            .collect();
        let changed_sources = new
            .sources
            .iter()
            .filter(|new_source| {
                old.sources
                    .iter()
                    .find(|old_source| old_source.id == new_source.id)
                    .map(|old_source| old_source != *new_source)
                    .unwrap_or(false)
            })
            .map(|s| s.id.clone())
            .collect();

        *self.snapshot.write().map_err(lock_error)? = new;

        Ok(ConfigDiff {
            added_sources,
            changed_sources,
            removed_sources,
        })
    }

    fn get_app_settings(&self) -> Result<AppSettings, ConfigError> {
        Ok(self
            .snapshot
            .read()
            .map_err(lock_error)?
            .app_settings
            .clone())
    }

    fn put_app_settings(&self, settings: &AppSettings) -> Result<(), ConfigError> {
        let existing = read_app_toml(&self.config_root)?;
        let app_toml = AppToml::from_app_settings(settings, &existing);
        write_app_toml(&self.config_root, &app_toml)?;
        self.snapshot.write().map_err(lock_error)?.app_settings = settings.clone();
        Ok(())
    }

    fn list_sources(&self) -> Result<Vec<AccountSettings>, ConfigError> {
        Ok(self.snapshot.read().map_err(lock_error)?.sources.clone())
    }

    fn get_source(&self, id: &AccountId) -> Result<Option<AccountSettings>, ConfigError> {
        Ok(self
            .snapshot
            .read()
            .map_err(lock_error)?
            .sources
            .iter()
            .find(|s| &s.id == id)
            .cloned())
    }

    fn save_source(&self, source: &AccountSettings) -> Result<(), ConfigError> {
        let source_toml = SourceToml::from_account_settings(source);
        let toml_str =
            toml::to_string_pretty(&source_toml).map_err(|e| ConfigError::Parse(e.to_string()))?;
        let path = self
            .config_root
            .join("sources")
            .join(format!("{}.toml", source.id));
        atomic_write(&path, toml_str.as_bytes())?;

        let mut snapshot = self.snapshot.write().map_err(lock_error)?;
        if let Some(existing) = snapshot.sources.iter_mut().find(|s| s.id == source.id) {
            *existing = source.clone();
        } else {
            snapshot.sources.push(source.clone());
        }
        Ok(())
    }

    fn delete_source(&self, id: &AccountId) -> Result<(), ConfigError> {
        let path = self
            .config_root
            .join("sources")
            .join(format!("{id}.toml"));
        if path.exists() {
            fs::remove_file(&path).map_err(io_error)?;
        }
        self.snapshot
            .write()
            .map_err(lock_error)?
            .sources
            .retain(|s| &s.id != id);
        Ok(())
    }

    fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError> {
        Ok(self
            .snapshot
            .read()
            .map_err(lock_error)?
            .smart_mailboxes
            .clone())
    }

    fn get_smart_mailbox(
        &self,
        id: &SmartMailboxId,
    ) -> Result<Option<SmartMailbox>, ConfigError> {
        Ok(self
            .snapshot
            .read()
            .map_err(lock_error)?
            .smart_mailboxes
            .iter()
            .find(|m| &m.id == id)
            .cloned())
    }

    fn save_smart_mailbox(&self, mailbox: &SmartMailbox) -> Result<(), ConfigError> {
        write_smart_mailbox_toml(&self.config_root, mailbox)?;

        let mut snapshot = self.snapshot.write().map_err(lock_error)?;
        if let Some(existing) = snapshot
            .smart_mailboxes
            .iter_mut()
            .find(|m| m.id == mailbox.id)
        {
            *existing = mailbox.clone();
        } else {
            snapshot.smart_mailboxes.push(mailbox.clone());
        }
        Ok(())
    }

    fn delete_smart_mailbox(&self, id: &SmartMailboxId) -> Result<(), ConfigError> {
        let path = self
            .config_root
            .join("smart-mailboxes")
            .join(format!("{id}.toml"));
        if path.exists() {
            fs::remove_file(&path).map_err(io_error)?;
        }
        self.snapshot
            .write()
            .map_err(lock_error)?
            .smart_mailboxes
            .retain(|m| &m.id != id);
        Ok(())
    }

    fn reset_default_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError> {
        let defaults = default_smart_mailboxes();
        let now = now_iso8601();
        for mailbox in &defaults {
            let mut with_timestamp = mailbox.clone();
            with_timestamp.updated_at = now.clone();
            write_smart_mailbox_toml(&self.config_root, &with_timestamp)?;
        }

        let mut snapshot = self.snapshot.write().map_err(lock_error)?;
        for default in &defaults {
            if let Some(existing) = snapshot
                .smart_mailboxes
                .iter_mut()
                .find(|m| m.id == default.id)
            {
                *existing = default.clone();
                existing.updated_at = now.clone();
            } else {
                let mut new = default.clone();
                new.updated_at = now.clone();
                snapshot.smart_mailboxes.push(new);
            }
        }

        // Sort by position
        snapshot
            .smart_mailboxes
            .sort_by(|a, b| a.position.cmp(&b.position).then(a.name.cmp(&b.name)));

        Ok(snapshot.smart_mailboxes.clone())
    }
}

// -- File I/O helpers --

fn load_snapshot_from_disk(config_root: &Path) -> Result<ConfigSnapshot, ConfigError> {
    let app_settings = read_app_toml(config_root)
        .map(|app| app.to_app_settings())
        .unwrap_or_default();

    let sources = load_sources(config_root)?;
    let smart_mailboxes = load_smart_mailboxes(config_root)?;

    Ok(ConfigSnapshot {
        app_settings,
        sources,
        smart_mailboxes,
    })
}

fn read_app_toml(config_root: &Path) -> Result<AppToml, ConfigError> {
    let path = config_root.join("app.toml");
    if !path.exists() {
        return Ok(AppToml::default());
    }
    let content = fs::read_to_string(&path).map_err(io_error)?;
    toml::from_str(&content).map_err(|e| ConfigError::Parse(format!("app.toml: {e}")))
}

fn write_app_toml(config_root: &Path, app: &AppToml) -> Result<(), ConfigError> {
    let content =
        toml::to_string_pretty(app).map_err(|e| ConfigError::Parse(e.to_string()))?;
    atomic_write(&config_root.join("app.toml"), content.as_bytes())
}

fn load_sources(config_root: &Path) -> Result<Vec<AccountSettings>, ConfigError> {
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
            sources.push(source.to_account_settings());
        }
    }
    sources.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(sources)
}

fn load_smart_mailboxes(config_root: &Path) -> Result<Vec<SmartMailbox>, ConfigError> {
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

fn write_smart_mailbox_toml(
    config_root: &Path,
    mailbox: &SmartMailbox,
) -> Result<(), ConfigError> {
    let toml_struct = SmartMailboxToml::from_smart_mailbox(mailbox);
    let content = toml::to_string_pretty(&toml_struct)
        .map_err(|e| ConfigError::Parse(e.to_string()))?;
    let path = config_root
        .join("smart-mailboxes")
        .join(format!("{}.toml", mailbox.id));
    atomic_write(&path, content.as_bytes())
}

fn validate_filename_matches_id(path: &Path, id: &str) -> Result<(), ConfigError> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if stem != id {
        return Err(ConfigError::Validation(format!(
            "filename '{}' does not match id '{id}' in {}",
            stem,
            path.display()
        )));
    }
    Ok(())
}

fn now_iso8601() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn io_error(err: std::io::Error) -> ConfigError {
    ConfigError::Io(err.to_string())
}

fn lock_error<T>(_: T) -> ConfigError {
    ConfigError::Io("config lock poisoned".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_root() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "mail-config-test-{}-{}-{}",
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

    #[test]
    fn empty_config_root_creates_empty_snapshot() {
        let root = temp_root();
        let repo = TomlConfigRepository::open(&root).unwrap();
        let snapshot = repo.load_snapshot().unwrap();

        assert!(snapshot.sources.is_empty());
        assert!(snapshot.smart_mailboxes.is_empty());
        assert_eq!(snapshot.app_settings, AppSettings::default());
    }

    #[test]
    fn initialize_defaults_creates_smart_mailbox_files() {
        let root = temp_root();
        let repo = TomlConfigRepository::open(&root).unwrap();
        repo.initialize_defaults().unwrap();

        assert!(root.join("app.toml").exists());
        assert!(root.join("smart-mailboxes/default-inbox.toml").exists());
        assert!(root.join("smart-mailboxes/default-all-mail.toml").exists());

        let snapshot = repo.load_snapshot().unwrap();
        assert_eq!(snapshot.smart_mailboxes.len(), 7);
    }

    #[test]
    fn source_crud_round_trips() {
        let root = temp_root();
        let repo = TomlConfigRepository::open(&root).unwrap();

        let source = AccountSettings {
            id: AccountId::from("test"),
            name: "Test".to_string(),
            driver: mail_domain::AccountDriver::Mock,
            enabled: true,
            transport: Default::default(),
            created_at: "2026-03-31T00:00:00Z".to_string(),
            updated_at: "2026-03-31T00:00:00Z".to_string(),
        };

        repo.save_source(&source).unwrap();
        assert!(root.join("sources/test.toml").exists());

        let loaded = repo.get_source(&AccountId::from("test")).unwrap().unwrap();
        assert_eq!(loaded.name, "Test");

        repo.delete_source(&AccountId::from("test")).unwrap();
        assert!(!root.join("sources/test.toml").exists());
        assert!(repo.get_source(&AccountId::from("test")).unwrap().is_none());
    }

    #[test]
    fn filename_id_mismatch_is_rejected() {
        let root = temp_root();
        let repo = TomlConfigRepository::open(&root).unwrap();

        // Write a source file with mismatched filename/id
        let bad_content = r#"
id = "real-id"
name = "Test"
driver = "mock"
enabled = true
"#;
        fs::write(root.join("sources/wrong-name.toml"), bad_content).unwrap();

        let result = repo.reload();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("wrong-name"), "error should mention filename: {err}");
        assert!(err.contains("real-id"), "error should mention id: {err}");
    }

    #[test]
    fn reload_detects_added_source() {
        let root = temp_root();
        let repo = TomlConfigRepository::open(&root).unwrap();

        // Externally write a source file
        let content = r#"
id = "new-source"
name = "New Source"
driver = "mock"
enabled = true
"#;
        fs::write(root.join("sources/new-source.toml"), content).unwrap();

        let diff = repo.reload().unwrap();
        assert_eq!(diff.added_sources, vec![AccountId::from("new-source")]);
        assert!(diff.removed_sources.is_empty());
        assert!(diff.changed_sources.is_empty());
    }

    #[test]
    fn smart_mailbox_crud_round_trips() {
        let root = temp_root();
        let repo = TomlConfigRepository::open(&root).unwrap();

        let mailbox = default_smart_mailboxes().into_iter().next().unwrap();
        repo.save_smart_mailbox(&mailbox).unwrap();

        let loaded = repo.get_smart_mailbox(&mailbox.id).unwrap().unwrap();
        assert_eq!(loaded.name, mailbox.name);
        assert_eq!(loaded.rule, mailbox.rule);

        repo.delete_smart_mailbox(&mailbox.id).unwrap();
        assert!(repo.get_smart_mailbox(&mailbox.id).unwrap().is_none());
    }
}
