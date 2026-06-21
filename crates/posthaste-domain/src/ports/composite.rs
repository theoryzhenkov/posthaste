use super::*;

/// Local store for synced mail data, events, and projections.
///
/// The store is the single source of truth for the UI -- the frontend reads
/// via the REST API, never directly from JMAP.
///
/// @spec docs/L1-sync#sqlite-schema
pub trait MailStore:
    MailboxReadStore
    + MailboxRoleOverrideStore
    + MessageListStore
    + TagReadStore
    + ConversationReadStore
    + MessageDetailStore
    + SmartMailboxStore
    + SyncStateStore
    + ImapSyncStateStore
    + ImapMessageLocationStore
    + MessageMailboxStore
    + SyncWriteStore
    + CacheStore
    + MessageCommandStore
    + EventStore
    + SourceProjectionStore
    + SourceDataStore
    + SenderAddressCacheStore
    + AutomationBackfillStore
    + OperationOutboxStore
{
}

impl<T> MailStore for T where
    T: MailboxReadStore
        + MailboxRoleOverrideStore
        + MessageListStore
        + TagReadStore
        + ConversationReadStore
        + MessageDetailStore
        + SmartMailboxStore
        + SyncStateStore
        + ImapSyncStateStore
        + ImapMessageLocationStore
        + MessageMailboxStore
        + SyncWriteStore
        + CacheStore
        + MessageCommandStore
        + EventStore
        + SourceProjectionStore
        + SourceDataStore
        + SenderAddressCacheStore
        + AutomationBackfillStore
        + OperationOutboxStore
{
}

/// Credential storage abstraction (OS keyring or environment variables).
///
/// @spec docs/L0-accounts#credential-storage
/// @spec docs/L1-api#secret-management
pub trait SecretStore: Send + Sync {
    /// Resolve a secret reference to its plaintext value.
    fn resolve(&self, secret_ref: &SecretRef) -> Result<String, SecretStoreError>;
    /// Store a new secret.
    fn save(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError>;
    /// Replace an existing secret's value.
    fn update(&self, secret_ref: &SecretRef, value: &str) -> Result<(), SecretStoreError>;
    /// Delete a stored secret.
    fn delete(&self, secret_ref: &SecretRef) -> Result<(), SecretStoreError>;
}

/// Thread-safe handle to a [`MailGateway`] implementation.
pub type SharedGateway = Arc<dyn MailGateway>;
/// Thread-safe handle to a [`SecretStore`] implementation.
pub type SharedSecretStore = Arc<dyn SecretStore>;

/// Extension trait for converting `Option<T>` into a not-found [`ServiceError`].
pub trait ServiceResultExt<T> {
    fn not_found(self, kind: &str, id: &str) -> Result<T, ServiceError>;
}

impl<T> ServiceResultExt<T> for Option<T> {
    fn not_found(self, kind: &str, id: &str) -> Result<T, ServiceError> {
        self.ok_or_else(|| StoreError::NotFound(format!("{kind}:{id}")).into())
    }
}
