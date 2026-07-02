use std::fs;

use posthaste_domain_service::{
    AccountId, AccountSettings, AppSettings, ConfigDiff, ConfigError, ConfigRepository,
    ConfigSnapshot, SmartMailbox, SmartMailboxId,
};

use crate::defaults::default_smart_mailboxes;
use crate::schema::AppToml;

use super::io::{
    io_error, load_snapshot_from_disk, lock_error, now_iso8601, read_app_toml, validate_safe_id,
    write_app_toml, write_smart_mailbox_toml, write_source_toml,
};
use super::TomlConfigRepository;

impl ConfigRepository for TomlConfigRepository {
    /// Returns a clone of the cached in-memory snapshot (no disk I/O).
    ///
    /// @spec docs/L1-accounts#configsnapshot
    fn load_snapshot(&self) -> Result<ConfigSnapshot, ConfigError> {
        Ok(self.snapshot.read().map_err(lock_error)?.clone())
    }

    /// Re-reads all files from disk, diffs against the cached snapshot, and
    /// returns `ConfigDiff` listing added/changed/removed sources.
    ///
    /// @spec docs/L1-accounts#configdiff
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

    /// Returns global app settings from the cached snapshot.
    fn get_app_settings(&self) -> Result<AppSettings, ConfigError> {
        Ok(self
            .snapshot
            .read()
            .map_err(lock_error)?
            .app_settings
            .clone())
    }

    /// Persists global app settings via atomic write and updates the snapshot.
    fn put_app_settings(&self, settings: &AppSettings) -> Result<(), ConfigError> {
        let existing = read_app_toml(&self.config_root)?;
        let app_toml = AppToml::from_app_settings(settings, &existing);
        write_app_toml(&self.config_root, &app_toml)?;
        self.snapshot.write().map_err(lock_error)?.app_settings = settings.clone();
        Ok(())
    }

    /// Lists account sources in the user's sidebar order: ids pinned in
    /// `account_order` first, then the rest by name (the fallback).
    fn list_sources(&self) -> Result<Vec<AccountSettings>, ConfigError> {
        let snapshot = self.snapshot.read().map_err(lock_error)?;
        let mut sources = snapshot.sources.clone();
        sources.sort_by(|a, b| a.name.cmp(&b.name));
        let order: Vec<&str> = snapshot
            .app_settings
            .account_order
            .iter()
            .map(|id| id.as_str())
            .collect();
        Ok(posthaste_domain_service::apply_explicit_order(
            sources,
            &order,
            |source| source.id.as_str(),
        ))
    }

    /// Looks up a single account source by ID from the cached snapshot.
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

    /// Creates an account source file via atomic write and updates the snapshot.
    /// Duplicate IDs are rejected before writing so account creation cannot
    /// overwrite existing source config.
    ///
    /// @spec docs/L1-accounts#id-validation
    fn insert_source(&self, source: &AccountSettings) -> Result<(), ConfigError> {
        validate_safe_id(source.id.as_str())?;
        let path = self
            .config_root
            .join("sources")
            .join(format!("{}.toml", source.id));

        let mut snapshot = self.snapshot.write().map_err(lock_error)?;
        if snapshot
            .sources
            .iter()
            .any(|existing| existing.id == source.id)
            || path.exists()
        {
            return Err(ConfigError::Conflict(format!(
                "account '{}' already exists",
                source.id
            )));
        }

        write_source_toml(&self.config_root, source)?;
        snapshot.sources.push(source.clone());
        Ok(())
    }

    /// Creates or updates an account source file via atomic write and updates
    /// the snapshot.
    ///
    /// @spec docs/L1-accounts#id-validation
    fn save_source(&self, source: &AccountSettings) -> Result<(), ConfigError> {
        validate_safe_id(source.id.as_str())?;
        write_source_toml(&self.config_root, source)?;

        let mut snapshot = self.snapshot.write().map_err(lock_error)?;
        if let Some(existing) = snapshot.sources.iter_mut().find(|s| s.id == source.id) {
            *existing = source.clone();
        } else {
            snapshot.sources.push(source.clone());
        }
        Ok(())
    }

    /// Deletes the source TOML file and removes the source from the snapshot.
    fn delete_source(&self, id: &AccountId) -> Result<(), ConfigError> {
        let path = self.config_root.join("sources").join(format!("{id}.toml"));
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

    /// Lists all smart mailboxes from the cached snapshot.
    /// Lists smart mailboxes in the user's sidebar order: ids pinned in
    /// `smart_mailbox_order` first, then the rest by the canonical fallback
    /// (built-ins in default order, then user mailboxes by created_at/name).
    fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError> {
        let snapshot = self.snapshot.read().map_err(lock_error)?;
        let mut mailboxes = snapshot.smart_mailboxes.clone();
        mailboxes.sort_by(|a, b| {
            posthaste_domain_service::smart_mailbox_fallback_rank(a.default_key.as_deref())
                .cmp(&posthaste_domain_service::smart_mailbox_fallback_rank(
                    b.default_key.as_deref(),
                ))
                .then_with(|| a.created_at.cmp(&b.created_at))
                .then_with(|| a.name.cmp(&b.name))
        });
        let order: Vec<&str> = snapshot
            .app_settings
            .smart_mailbox_order
            .iter()
            .map(|id| id.as_str())
            .collect();
        Ok(posthaste_domain_service::apply_explicit_order(
            mailboxes,
            &order,
            |mailbox| mailbox.id.as_str(),
        ))
    }

    /// Looks up a single smart mailbox by ID from the cached snapshot.
    fn get_smart_mailbox(&self, id: &SmartMailboxId) -> Result<Option<SmartMailbox>, ConfigError> {
        Ok(self
            .snapshot
            .read()
            .map_err(lock_error)?
            .smart_mailboxes
            .iter()
            .find(|m| &m.id == id)
            .cloned())
    }

    /// Creates or updates a smart mailbox TOML file and updates the snapshot.
    ///
    /// @spec docs/L1-accounts#id-validation
    fn save_smart_mailbox(&self, mailbox: &SmartMailbox) -> Result<(), ConfigError> {
        validate_safe_id(mailbox.id.as_str())?;
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

    /// Deletes the smart mailbox TOML file and removes it from the snapshot.
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

    /// Restores built-in default smart mailboxes by upserting them. Existing
    /// user-created mailboxes are preserved.
    ///
    /// @spec docs/L1-accounts#smart-mailbox-defaults
    fn reset_default_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError> {
        let defaults = default_smart_mailboxes();
        let now = now_iso8601();
        for mailbox in &defaults {
            let mut with_timestamp = mailbox.clone();
            with_timestamp.updated_at.clone_from(&now);
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
                existing.updated_at.clone_from(&now);
            } else {
                let mut new = default.clone();
                new.updated_at.clone_from(&now);
                snapshot.smart_mailboxes.push(new);
            }
        }

        // Canonical fallback order (built-ins first); the user's explicit
        // arrangement is applied at read time in `list_smart_mailboxes`.
        snapshot.smart_mailboxes.sort_by(|a, b| {
            posthaste_domain_service::smart_mailbox_fallback_rank(a.default_key.as_deref())
                .cmp(&posthaste_domain_service::smart_mailbox_fallback_rank(
                    b.default_key.as_deref(),
                ))
                .then_with(|| a.created_at.cmp(&b.created_at))
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok(snapshot.smart_mailboxes.clone())
    }
}
