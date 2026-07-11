use super::*;
use posthaste_domain_model::MessageRecord;

/// The optimistic OVERLAY plane's storage port (NS1, D167/D169).
///
/// Rows written through this port are the FOLD'S OUTPUT: complete folded
/// message projections computed by the shared replica-core fold — never
/// partial deltas. An overlaid message takes its row, mailbox membership and
/// keywords entirely from the overlay; the `*_effective` SQL views merge this
/// plane over the sync-owned base tables for every SQL read (D168).
///
/// One writer per plane: sync writes base ONLY (`SyncWriteStore`); the fold
/// engine writes the overlay ONLY (this port). Lifecycle mirrors the pending
/// set's retire-on-confirmation:
///   accept  → [`upsert_overlay_message`] (folded row) or
///             [`tombstone_overlay_message`] (pending Destroy),
///   refold  → [`upsert_overlay_message`] again (base changed under a pending
///             effect; the fold recomputed),
///   retire  → [`remove_overlay_message`] (the reconciler observed the effect
///             in base; the base row shows through again).
///
/// NOTE (NS1 substrate increment): defined and store-implemented ahead of the
/// fold-engine wiring. Nothing writes the overlay in production yet — every
/// `*_effective` view is currently identical to its base table.
///
/// @spec docs/eph/RFC-L2-client-replication-model#6-the-runtime-substrate-base--overlay--effective-d167d169
pub trait MessageOverlayStore: Send + Sync {
    /// Write (or refresh) the complete folded projection for one message:
    /// the row plus its full mailbox and keyword sets, atomically. Clears any
    /// prior tombstone for the id.
    fn upsert_overlay_message(
        &self,
        account_id: &AccountId,
        message: &MessageRecord,
    ) -> Result<(), StoreError>;

    /// Mark a pending optimistic Destroy: the message is hidden from
    /// `message_effective` (and its sets emptied) while base still holds it.
    fn tombstone_overlay_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), StoreError>;

    /// Retire the overlay entry (the reconciler observed the folded effect in
    /// base, or the op reverted): the base row shows through again. Removing
    /// an id with no overlay entry is a no-op.
    fn remove_overlay_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<(), StoreError>;

    /// The ids currently overlaid for an account (tombstoned included) —
    /// the fold engine's reseed/reconcile inventory.
    fn list_overlay_message_ids(
        &self,
        account_id: &AccountId,
    ) -> Result<Vec<MessageId>, StoreError>;
}

/// Object-safety guard: the service composes this port dynamically.
const _: fn(&dyn MessageOverlayStore) = |_| {};
