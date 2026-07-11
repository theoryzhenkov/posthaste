use super::*;

/// Sync cursor state boundary.
pub trait SyncStateStore: Send + Sync {
    /// Load all stored sync cursors for an account.
    ///
    /// @spec docs/L1-sync#state-management
    fn get_sync_cursors(&self, account_id: &AccountId) -> Result<Vec<SyncCursor>, StoreError>;

    /// Load a single sync cursor by object type.
    ///
    /// @spec docs/L1-sync#state-management
    fn get_cursor(
        &self,
        account_id: &AccountId,
        object_type: SyncObject,
    ) -> Result<Option<SyncCursor>, StoreError>;
}

/// IMAP per-mailbox sync cursor read boundary.
///
/// @spec docs/L0-providers#imap-cursors-per-mailbox
pub trait ImapSyncStateStore: Send + Sync {
    fn list_imap_mailbox_states(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<ImapMailboxSyncState>, StoreError>;

    fn get_imap_mailbox_state(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
    ) -> Result<Option<ImapMailboxSyncState>, StoreError>;
}

/// IMAP per-mailbox sync cursor write boundary.
///
/// @spec docs/L0-providers#imap-cursors-per-mailbox
pub trait ImapSyncStateWriteStore: Send + Sync {
    fn put_imap_mailbox_state(
        &self,
        account_id: &AccountId,
        state: &ImapMailboxSyncState,
    ) -> Result<(), StoreError>;

    fn delete_imap_mailbox_state(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
    ) -> Result<(), StoreError>;
}

/// IMAP message location read boundary.
///
/// @spec docs/L0-providers#identity-and-threading
pub trait ImapMessageLocationStore: Send + Sync {
    fn list_imap_message_locations(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Vec<ImapMessageLocation>, StoreError>;

    fn list_imap_mailbox_message_locations(
        &self,
        account_id: &AccountId,
        mailbox_id: &MailboxId,
    ) -> Result<Vec<ImapMessageLocation>, StoreError>;
}

/// IMAP message location write boundary.
///
/// @spec docs/L0-providers#identity-and-threading
pub trait ImapMessageLocationWriteStore: Send + Sync {
    fn put_imap_message_location(
        &self,
        account_id: &AccountId,
        location: &ImapMessageLocation,
    ) -> Result<(), StoreError>;

    fn delete_imap_message_locations(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), StoreError>;
}

/// Message mailbox membership read boundary.
pub trait MessageMailboxStore: Send + Sync {
    /// Return current mailbox memberships for a message.
    ///
    /// @spec docs/L1-sync#sqlite-schema
    fn get_message_mailboxes(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Vec<MailboxId>, StoreError>;
}

/// Sync batch and lazy body write boundary.
///
/// Stays a plain synchronous `&self` trait (not `async`, D63/M23b design
/// note): `posthaste-store`'s own unit test suite calls these methods
/// directly on a bare `DatabaseStore` with no `Arc` and no tokio runtime
/// (`store.rs`'s doc on `write_transaction`/`read_connection` preserves that
/// exact invariant for the primitives these build on) — making the port
/// `async` would force every one of those tests to acquire a runtime. Instead,
/// the async offload lives at the call site: every production caller reaches
/// these through `Arc<dyn SyncWriteStore>` (`Send + Sync + 'static`) and, when
/// called from an async context, wraps the call in `tokio::task::spawn_blocking`
/// (see `MailService`'s call sites, e.g. `ServiceSyncSink::emit`) so the SQLite
/// work — the heaviest write on the sync path — never occupies a tokio worker
/// thread, without forcing the port itself to be async.
///
/// @spec docs/eph/RFC-L2-lifecycle-and-errors#d63
pub trait SyncWriteStore: Send + Sync {
    /// Apply a sync batch atomically within a single SQLite transaction.
    ///
    /// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
    fn apply_sync_batch(
        &self,
        account_id: &AccountId,
        batch: &SyncBatch,
    ) -> Result<Vec<DomainEvent>, StoreError>;

    /// Run the final reconciliation pass for a streamed upsert-only sync:
    /// prune locals absent from the complete remote id set and commit the
    /// withheld cursors, atomically. Only invoked when the gateway streamed
    /// chunks and returned a reconciliation set; a single self-reconciling
    /// batch skips it.
    ///
    fn reconcile_sync(
        &self,
        account_id: &AccountId,
        reconciliation: &SyncReconciliation,
    ) -> Result<Vec<DomainEvent>, StoreError>;

    /// Persist a lazily-fetched message body.
    ///
    /// @spec docs/L1-sync#body-lazy
    fn apply_message_body(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        body: &FetchedBody,
    ) -> Result<CommandResult, StoreError>;
}
