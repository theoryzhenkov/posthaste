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
    + ImapMessageLocationWriteStore
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
    + DraftRegistry
    + RevLogStore
    + SnoozeStore
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
        + ImapMessageLocationWriteStore
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
        + DraftRegistry
        + RevLogStore
        + SnoozeStore
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

    /// Compare-and-swap an existing secret's value (D101 / A1): replace the
    /// stored value with `new_value` **only if** it currently equals
    /// `expected_current`. When the stored value has drifted — a concurrent
    /// writer rotated it out from under this caller — nothing is written and
    /// [`SecretCasOutcome::Mismatch`] is returned carrying the *current* stored
    /// value so the caller can adopt the winner instead of clobbering it with a
    /// stale (potentially already-consumed) token set.
    ///
    /// This is the rotation-safe replacement for a blind [`Self::update`] on the
    /// OAuth refresh path: two racing refreshes that both read token set `A`,
    /// POST it, and get back rotated sets `B`/`B'` can no longer last-writer-wins
    /// one over the other — the loser's CAS misses and it re-reads the winner's
    /// token rather than persisting a consumed refresh token (permanent
    /// `invalid_grant` lockout).
    ///
    /// The default implementation is a plain read-compare-write and is therefore
    /// only as atomic as the backing store plus whatever external serialization
    /// the caller holds. An implementation over a backing without a native CAS
    /// primitive (keyring, env) should guard the read-compare-write in a
    /// process-local critical section and document the residual cross-process
    /// window (see `SystemSecretStore`).
    fn update_if_unchanged(
        &self,
        secret_ref: &SecretRef,
        expected_current: &str,
        new_value: &str,
    ) -> Result<SecretCasOutcome, SecretStoreError> {
        let current = self.resolve(secret_ref)?;
        if current != expected_current {
            return Ok(SecretCasOutcome::Mismatch { current });
        }
        self.update(secret_ref, new_value)?;
        Ok(SecretCasOutcome::Swapped)
    }
}

/// Outcome of a [`SecretStore::update_if_unchanged`] compare-and-swap.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SecretCasOutcome {
    /// The stored value equalled `expected_current` and was replaced.
    Swapped,
    /// The stored value had drifted from `expected_current`; nothing was
    /// written. Carries the *current* stored value (the winning writer's) so the
    /// caller can adopt it rather than re-deriving or clobbering it.
    Mismatch { current: String },
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
