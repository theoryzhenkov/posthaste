use super::*;

impl MailService {
    // -- Config delegates --

    /// Read global application settings.
    ///
    /// @spec docs/L1-api#settings
    pub fn get_app_settings(&self) -> Result<AppSettings, ServiceError> {
        self.config.get_app_settings().map_err(Into::into)
    }

    /// Persist updated global application settings.
    ///
    /// @spec docs/L1-api#settings
    pub fn put_app_settings(&self, settings: &AppSettings) -> Result<(), ServiceError> {
        self.config.put_app_settings(settings).map_err(Into::into)
    }

    /// List all account configurations.
    ///
    /// @spec docs/L1-api#accounts
    pub fn list_sources(&self) -> Result<Vec<AccountSettings>, ServiceError> {
        self.config.list_sources().map_err(Into::into)
    }

    /// Look up a single account configuration by ID.
    pub fn get_source(&self, id: &AccountId) -> Result<Option<AccountSettings>, ServiceError> {
        self.config.get_source(id).map_err(Into::into)
    }

    /// Create an account, rejecting duplicate IDs, and sync the source projection in the store.
    ///
    /// @spec docs/L1-api#account-crud-lifecycle
    pub fn insert_source(&self, source: &AccountSettings) -> Result<(), ServiceError> {
        self.config.insert_source(source)?;
        self.source_projections
            .upsert_source_projection(&source.id, &source.name)?;
        Ok(())
    }

    /// Create or update an account, syncing the source projection in the store.
    ///
    /// @spec docs/L1-api#account-crud-lifecycle
    pub fn save_source(&self, source: &AccountSettings) -> Result<(), ServiceError> {
        self.config.save_source(source)?;
        self.source_projections
            .upsert_source_projection(&source.id, &source.name)?;
        Ok(())
    }

    /// Delete an account: remove config, projection, and all synced data.
    ///
    /// @spec docs/L1-api#account-crud-lifecycle
    pub fn delete_source(&self, id: &AccountId) -> Result<(), ServiceError> {
        let mut settings = self.config.get_app_settings()?;
        if settings.default_account_id.as_ref() == Some(id) {
            settings.default_account_id = None;
            self.config.put_app_settings(&settings)?;
        }
        self.config.delete_source(id)?;
        self.source_projections.delete_source_projection(id)?;
        self.source_data.delete_source_data(id)?;
        Ok(())
    }

    /// List smart mailbox configurations (without live counts).
    pub fn list_smart_mailboxes_config(&self) -> Result<Vec<SmartMailbox>, ServiceError> {
        self.config.list_smart_mailboxes().map_err(Into::into)
    }

    /// Fetch a single smart mailbox configuration, or 404.
    pub fn get_smart_mailbox(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<SmartMailbox, ServiceError> {
        self.config
            .get_smart_mailbox(smart_mailbox_id)?
            .not_found("smart_mailbox", smart_mailbox_id.as_str())
    }

    /// Create or update a smart mailbox configuration.
    ///
    /// @spec docs/L1-api#smart-mailbox-crud
    pub fn save_smart_mailbox(&self, smart_mailbox: &SmartMailbox) -> Result<(), ServiceError> {
        self.config
            .save_smart_mailbox(smart_mailbox)
            .map_err(Into::into)
    }

    /// Delete a smart mailbox configuration.
    pub fn delete_smart_mailbox(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<(), ServiceError> {
        self.config
            .delete_smart_mailbox(smart_mailbox_id)
            .map_err(Into::into)
    }

    /// Restore all default smart mailboxes, preserving user-created ones.
    ///
    /// @spec docs/L1-accounts#smart-mailbox-defaults
    pub fn reset_default_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ServiceError> {
        self.config
            .reset_default_smart_mailboxes()
            .map_err(Into::into)
    }

    /// Re-read config from disk, diff it, and sync source projections.
    ///
    /// @spec docs/L1-accounts#configdiff
    pub fn reload_config(&self) -> Result<ConfigDiff, ServiceError> {
        let diff = self.config.reload()?;
        for source_id in &diff.removed_sources {
            self.source_projections
                .delete_source_projection(source_id)?;
            self.source_data.delete_source_data(source_id)?;
        }
        // Sync all source projections after reload
        self.sync_source_projections()?;
        Ok(diff)
    }

    /// Upsert source projection rows for all configured accounts.
    pub fn sync_source_projections(&self) -> Result<(), ServiceError> {
        let sources = self.config.list_sources()?;
        for source in &sources {
            self.source_projections
                .upsert_source_projection(&source.id, &source.name)?;
        }
        Ok(())
    }
}
