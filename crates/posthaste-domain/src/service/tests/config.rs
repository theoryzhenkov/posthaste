use super::*;

pub(super) struct TestConfig {
    pub(super) smart_mailboxes: Vec<SmartMailbox>,
    pub(super) sources: Vec<AccountSettings>,
    pub(super) reload_diff: ConfigDiff,
    pub(super) app_settings: Mutex<AppSettings>,
    pub(super) deleted_sources: Mutex<Vec<AccountId>>,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            smart_mailboxes: Vec::new(),
            sources: Vec::new(),
            reload_diff: ConfigDiff {
                added_sources: Vec::new(),
                changed_sources: Vec::new(),
                removed_sources: Vec::new(),
            },
            app_settings: Mutex::new(AppSettings::default()),
            deleted_sources: Mutex::new(Vec::new()),
        }
    }
}

impl ConfigRepository for TestConfig {
    fn load_snapshot(&self) -> Result<ConfigSnapshot, ConfigError> {
        Ok(ConfigSnapshot {
            app_settings: self.get_app_settings()?,
            sources: self.sources.clone(),
            smart_mailboxes: self.smart_mailboxes.clone(),
        })
    }

    fn reload(&self) -> Result<ConfigDiff, ConfigError> {
        Ok(self.reload_diff.clone())
    }

    fn get_app_settings(&self) -> Result<AppSettings, ConfigError> {
        Ok(self
            .app_settings
            .lock()
            .expect("app settings lock poisoned")
            .clone())
    }

    fn put_app_settings(&self, settings: &AppSettings) -> Result<(), ConfigError> {
        *self
            .app_settings
            .lock()
            .expect("app settings lock poisoned") = settings.clone();
        Ok(())
    }

    fn list_sources(&self) -> Result<Vec<AccountSettings>, ConfigError> {
        Ok(self.sources.clone())
    }

    fn get_source(&self, id: &AccountId) -> Result<Option<AccountSettings>, ConfigError> {
        Ok(self.sources.iter().find(|source| &source.id == id).cloned())
    }

    fn save_source(&self, _source: &AccountSettings) -> Result<(), ConfigError> {
        Ok(())
    }

    fn delete_source(&self, id: &AccountId) -> Result<(), ConfigError> {
        self.deleted_sources
            .lock()
            .expect("deleted sources lock poisoned")
            .push(id.clone());
        Ok(())
    }

    fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError> {
        Ok(self.smart_mailboxes.clone())
    }

    fn get_smart_mailbox(&self, id: &SmartMailboxId) -> Result<Option<SmartMailbox>, ConfigError> {
        Ok(self
            .smart_mailboxes
            .iter()
            .find(|mailbox| &mailbox.id == id)
            .cloned())
    }

    fn save_smart_mailbox(&self, _mailbox: &SmartMailbox) -> Result<(), ConfigError> {
        Ok(())
    }

    fn delete_smart_mailbox(&self, _id: &SmartMailboxId) -> Result<(), ConfigError> {
        Ok(())
    }

    fn reset_default_smart_mailboxes(&self) -> Result<Vec<SmartMailbox>, ConfigError> {
        Ok(self.smart_mailboxes.clone())
    }
}
