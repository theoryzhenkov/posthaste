use std::collections::HashMap;

use super::*;
use posthaste_domain_model::{MailboxId, MessageRecord, Operation};

/// The optimistic OVERLAY plane's storage port (NS1, D167/D169).
///
/// Rows written through this port are the FOLD'S OUTPUT: complete folded
/// message projections computed by the shared replica-core fold — never
/// partial deltas. An overlaid message takes its row, mailbox membership and
/// keywords entirely from the overlay; the `*_effective` SQL views merge this
/// plane over the sync-owned base tables for every SQL read (D168).
///
/// One writer per plane: sync writes base ONLY (`SyncWriteStore`); the fold
/// engine writes the overlay ONLY (this port). Lifecycle:
///   accept  → [`upsert_overlay_message`] (folded row) or
///             [`tombstone_overlay_message`] (pending Destroy),
///   refold  → [`upsert_overlay_message`] again (base changed under a pending
///             effect; the fold recomputed),
///   remove  → [`remove_overlay_message`] (the row's ops truncated out of the
///             log; the entry re-derives from base alone and vanishes).
///
/// The overlay is written in production by `derive_overlay` /
/// `remove_op_and_derive`: one write transaction snapshots base + the log +
/// the draft-key map, runs the service fold, applies the mutation, and
/// returns the visibility diff — atomic and pure.
///
/// Transaction-consistent inputs to one row's fold: the snapshot the store
/// captures inside ONE write transaction so the fold's decision and the
/// overlay write commit atomically. The fold reads ONLY from this snapshot
/// (plus its captured service config) — never the live store — so the
/// derived row is `replay(log, base)` evaluated at one commit point, with no
/// room for a concurrent base write or a sibling refresh to interleave
/// (SQLite serializes writers).
pub struct DeriveSnapshot {
    /// The base-plane row (raw provider truth), or `None` if base has none.
    pub base: Option<MessageRecord>,
    /// The current overlay entry: `None` = no entry; `Some(None)` = a
    /// tombstone (pending destroy); `Some(Some(record))` = a folded row.
    pub overlay: Option<Option<MessageRecord>>,
    /// The account's unsettled ops in log (`rowid`) order — the fold's
    /// replayable view of the log (failed content ops included as parked;
    /// failed intent ops excluded — base wins).
    pub ops: Vec<Operation>,
    /// The account's draft-key → live-entity-id map (the `draft_alias`
    /// table). Absent = unmapped → the fold treats the key as its own live
    /// id (the registry's self-map convention).
    pub draft_keys: HashMap<String, String>,
    /// The account's Drafts and Sent mailbox ids (resolved by role, inside the
    /// transaction from the `mailbox` table — NOT the account-wide unread/total
    /// aggregation). The fold needs them to file a draft's or a provisional
    /// Sent row; carrying them in the snapshot keeps the fold pure (no second
    /// connection, no aggregation in the writer lock).
    pub drafts_mailbox: Option<MailboxId>,
    pub sent_mailbox: Option<MailboxId>,
}

/// The fold's decision for one row. The store applies it inside the same
/// transaction that captured the snapshot, and computes the visibility diff.
/// `Upsert` is boxed so the enum stays small (`MessageRecord` is ~500 bytes;
/// the other variants carry nothing).
pub enum OverlayMutation {
    /// Write the folded row (clears any prior tombstone).
    Upsert(Box<MessageRecord>),
    /// A pending destroy: hide the row while base still holds it.
    Tombstone,
    /// Retire the overlay entry: base shows through.
    Remove,
    /// No change — e.g. a tombstone over a surviving base row keeps hiding it,
    /// or a plain assertion over no base row that was never overlaid.
    Keep,
}

/// The overlay's visibility transition the derive produced, so the caller can
/// emit the retire echo without a separate before/after read: a content op's
/// local/provisional id yielding to base is `was_visible && !now_visible`.
#[derive(Clone, Copy, Debug)]
pub struct DeriveDiff {
    pub was_visible: bool,
    pub now_visible: bool,
}

impl DeriveDiff {
    /// The derived row retired this derive — a content op's local/provisional
    /// id dropped, with no base row to replace it.
    pub fn retired(self) -> bool {
        self.was_visible && !self.now_visible
    }
}

/// The fold closure for one row's derive (see [`MessageOverlayStore::derive_overlay`]).
/// A type alias keeps the trait signature off clippy's `type_complexity` list.
pub type OverlayFold =
    Box<dyn FnOnce(&DeriveSnapshot) -> Result<OverlayMutation, StoreError> + Send>;

/// The fold closure for [`MessageOverlayStore::remove_op_and_derive`], called
/// once per touched row id.
pub type OverlayFoldMany =
    Box<dyn Fn(&MessageId, &DeriveSnapshot) -> Result<OverlayMutation, StoreError> + Send>;

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

    /// Read the BASE-plane record for one message (raw provider truth: the
    /// `message` row + base membership + base keywords — never the overlay).
    /// This is the fold's INPUT: `refresh` folds the unsettled ops over it and
    /// writes the result back through [`Self::upsert_overlay_message`]. Body
    /// fields are not loaded (`None`) — the fold never touches them.
    fn read_base_message_record(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<MessageRecord>, StoreError>;

    /// Read one overlay entry: `None` = no entry; `Some(None)` = a tombstone
    /// (pending destroy); `Some(Some(record))` = a folded row (keywords +
    /// mailbox sets loaded; body fields `None`). Drives the no-ops replay
    /// arm's ownerless-artifact pass-through (a pinned row with no base row;
    /// a tombstone over a surviving base row).
    fn read_overlay_message(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
    ) -> Result<Option<Option<MessageRecord>>, StoreError>;

    /// Find a BASE message whose `rfc_message_id` starts with `prefix` — the
    /// provisional Sent row's adoption probe (NS2 Slice 4,
    /// reconcile-by-intent-id): the transport-shared send identity
    /// (`phsend-<op>@`, [`posthaste_domain_model::send_identity_prefix`])
    /// matches the synced provider copy in any domain.
    fn find_base_message_id_by_rfc_prefix(
        &self,
        account_id: &AccountId,
        prefix: &str,
    ) -> Result<Option<MessageId>, StoreError>;

    /// Derive one row's overlay entry ATOMICALLY: capture `base` + the
    /// unsettled log + the draft-key map in ONE write transaction, call
    /// `fold` (which reads ONLY from the snapshot), apply the resulting
    /// mutation, and return the visibility diff.
    ///
    /// This is the single atomic unit of the derived plane — the overlay
    /// write commits with the snapshot it was folded from, so no concurrent
    /// base write (sync) or sibling refresh (another command) can interleave
    /// to produce a stale overlay (SQLite serializes writers; the fold is a
    /// pure function of the snapshot). The fold closure is `Send + 'static`:
    /// capture owned ids and `Arc`-cloned config readers, never `&self`.
    fn derive_overlay(
        &self,
        account_id: &AccountId,
        message_id: &MessageId,
        fold: OverlayFold,
    ) -> Result<DeriveDiff, StoreError>;

    /// Remove one op and re-derive every row it touched, atomically: the op
    /// removal and the overlay re-derivation commit in ONE transaction, so a
    /// crash between them cannot leave a derived row whose owning op is gone
    /// — the orphan the model claims is unrepresentable. `fold` is called once
    /// per row id with a snapshot taken AFTER the op's removal (so the removed
    /// op's effect is absent from the fold). Returns the visibility diff per
    /// row, in `row_ids` order, for the caller's retire echo.
    fn remove_op_and_derive(
        &self,
        account_id: &AccountId,
        op_id: &posthaste_domain_model::OperationId,
        row_ids: &[MessageId],
        fold: OverlayFoldMany,
    ) -> Result<Vec<DeriveDiff>, StoreError>;
}

/// Object-safety guard: the service composes this port dynamically.
const _: fn(&dyn MessageOverlayStore) = |_| {};
