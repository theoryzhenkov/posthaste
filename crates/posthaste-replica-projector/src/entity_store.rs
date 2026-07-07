//! The reactive entity store core (merged slices 2+3, sub-slices 2a + optimism).
//!
//! Generalizes [`crate::MailListReplica`] from a single mail-list view into a
//! normalized, keyed entity store with register-by-use / drain-dirty reactivity:
//! `message[id]` and `view[viewId]` (an ordered row list plus coverage). The
//! host feeds it authoritative batches — message mutations carrying the full
//! `MessageSummary` projection, per `firehose-carries-rows` — and the store
//! applies the whole batch atomically, then reports the keys that changed via
//! [`EntityStore::drain_dirty`]. The host does the reactive fan-out (write the
//! changed keys into the renderer cache); the store is a dumb dirty-tracker.
//!
//! [`EntityStore`] is the public **composition** of the two near-node layers
//! (RFC D36): the replica *mechanism* ([`crate::mechanism`] — accept/settle/
//! retire plumbing over replica-core's `OptimisticReplica` kernel) and the view
//! *projection* ([`crate::projection`] — rows, predicates, windowing, sort
//! keys, dirty tracking). A headless client consumes exactly these two layers.
//!
//! For an **evaluable** predicate (`InMailboxes`, `All`) the store self-maintains
//! each view's membership: on a message mutation it runs one local evaluation —
//! place the row if the predicate matches *and* the message's sort key is within
//! the held coverage `[TOP, W]`, otherwise ignore (or remove, if it was held and
//! no longer matches). A mutation can shrink or hold the range, never grow it
//! downward; only paging grows `W` (`mutations-absorb-or-ignore`,
//! `paging-grows-range`). A **deferred** predicate is never self-evaluated; the
//! host drives its membership via deltas.
//!
//! ## Optimism
//!
//! The mechanism layer holds a `MessageReplica` (the shared convergence engine,
//! `posthaste-replica-core`'s `Replica<MessageConvergence>`) over message fold
//! state. [`accept_mutation`](EntityStore::accept_mutation) folds an optimistic
//! assertion into the outbox; [`message`](EntityStore::message) and view
//! placement read the *projected* state — the confirmed base with pending
//! folded over it — so optimism is a general property of the store, never stored
//! as truth (`view-is-pure-fold`). Confirm retires the pending op (the served
//! base already carries the effect, so it is a visual no-op); a failed settle
//! drops it and the projection reverts to authoritative state. Pending survives
//! an unrelated base update (a sibling arrival re-seeds only its own base; the
//! outbox is untouched and re-folds).
//!
//! Counts are **not** here at all (RFC-L2-count-unification): the store is
//! partial, so a held-window count is never the true total, and the client
//! reads mailbox counts via react-query invalidation of the runtime's
//! canonical trigger-maintained mailbox rows — no per-event delta application.
//!
//! Coverage is held as the watermark `W` (the sort key of the last held row;
//! `None` = reaches BOTTOM); the full multi-range `CoverageRange` shape is
//! adopted when jump-to-date lands. The typed `SortKey` supports the
//! `[receivedAt, id]` composite (the default and overwhelmingly common sort);
//! views under a different sort are `Deferred`.
//!
//! @spec docs/eph/DESIGN-L2-client-link-reactive-store
//! @spec docs/eph/PLAN-L2-client-link-reactive-store

use serde::{Deserialize, Serialize};
use serde_json::Value;

use posthaste_replica_core::{MessageAssertion, MutationId, SettlementOutcome, SettlementResult};

use crate::mechanism::{BaseApplied, ReplicaMechanism};
use crate::projection::{DirtyKey, SortDirection, SortKey, ViewPredicate, ViewProjection, ViewRow};

/// One authoritative update in a batch.
///
/// Serializes externally-tagged + camelCase: `{"message":{messageId,
/// projection, deleted}}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StoreUpdate {
    /// A message mutation carrying the full projection (enough to evaluate
    /// membership, compute the sort key, and render) —
    /// `firehose-carries-rows`, `D2`.
    #[serde(rename_all = "camelCase")]
    Message {
        message_id: String,
        projection: Value,
        deleted: bool,
    },
}

/// The reactive entity store. Pure compute: no transport, no persistence.
///
/// Composes the replica mechanism (the shared convergence kernel behind
/// replica-core's `OptimisticReplica` seam) with the view projection layer, so
/// message optimism is a pure fold over the confirmed base (keywords + mailbox
/// membership); views and the message read derive from the projected state.
/// Bases are seeded per-key on ingest; the outbox is never cleared by a base
/// update, so unconfirmed optimism survives a re-served snapshot and retires
/// only on settlement.
#[derive(Default)]
pub struct EntityStore {
    mechanism: ReplicaMechanism,
    projection: ViewProjection,
}

impl EntityStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a view with its predicate, sort, and initial coverage watermark.
    /// The host calls this when a view is opened (or its window grows, updating
    /// the watermark). Marks the view dirty so the host re-reads its rows.
    pub fn register_view(
        &mut self,
        view_id: &str,
        predicate: ViewPredicate,
        sort_field: String,
        sort_direction: SortDirection,
        watermark: Option<SortKey>,
    ) {
        self.projection
            .register_view(view_id, predicate, sort_field, sort_direction, watermark);
    }

    /// Adopt a served snapshot / page / resync for a view — reconciled against
    /// the version-guarded optimistic state, not clobbered (see
    /// [`ViewProjection::set_view_rows`](crate::projection) for the full
    /// invariant). Does not touch the message base or outbox.
    pub fn set_view_rows(&mut self, view_id: &str, rows: Vec<ViewRow>, watermark: Option<SortKey>) {
        self.projection
            .set_view_rows(&self.mechanism, view_id, rows, watermark);
    }

    pub fn close_view(&mut self, view_id: &str) {
        self.projection.close_view(view_id);
    }

    /// Apply a batch atomically: every update is applied before any dirty key
    /// is reported, and subscribers see one drain. (`atomic-batch`.)
    pub fn ingest_batch(&mut self, updates: Vec<StoreUpdate>) {
        for update in updates {
            match update {
                StoreUpdate::Message {
                    message_id,
                    projection,
                    deleted,
                } => {
                    self.apply_message(&message_id, &projection, deleted);
                }
            }
        }
    }

    /// Accept an optimistic message mutation into the outbox (idempotent on
    /// mutation id). The projected state is re-derived for the affected message
    /// so reads and view membership reflect the fold immediately. A mutation on
    /// a message whose base has not yet been ingested is tracked but deferred —
    /// it folds in once the authoritative projection arrives.
    pub fn accept_mutation(
        &mut self,
        mutation_id: MutationId,
        message_id: &str,
        assertion: MessageAssertion,
    ) {
        self.mechanism.accept(mutation_id, message_id, assertion);
        if self.mechanism.is_held(message_id) {
            self.projection
                .rederive_message(&self.mechanism, message_id);
        }
    }

    /// Settle a pending mutation by its terminal outcome.
    ///
    /// `Confirmed` does **not** revert: it retires the op only if the confirmed
    /// base already carries its effect at a strictly-higher authority version
    /// (the kernel's version-gated retire), otherwise it leaves the op folded
    /// for the authoritative `message.updated` to retire. This is the race fix
    /// ([mutation.notification design](../eph/DESIGN-L2-mutation-notification.md)):
    /// a confirmation that outruns the base update can no longer flip the
    /// projection back to a stale base. `Failed` retires the op unconditionally
    /// and the projection reverts to authoritative state (the rejection path).
    /// Out-of-order safe; idempotent on an unknown id.
    pub fn settle(
        &mut self,
        mutation_id: &MutationId,
        outcome: SettlementOutcome,
    ) -> SettlementResult {
        let (result, key) = self.mechanism.settle(mutation_id, outcome);
        if let Some(message_id) = key {
            if self.mechanism.is_held(&message_id) {
                self.projection
                    .rederive_message(&self.mechanism, &message_id);
            }
        }
        result
    }

    /// Whether any optimistic mutation is still pending (drives optimistic-UI
    /// affordances and settle-driven dirty drains).
    pub fn has_pending(&self) -> bool {
        self.mechanism.has_pending()
    }

    fn apply_message(&mut self, message_id: &str, projection: &Value, deleted: bool) {
        match self.mechanism.apply_base(message_id, projection, deleted) {
            BaseApplied::RejectedStale => return,
            BaseApplied::Removed => self.projection.remove_message_from_views(message_id),
            BaseApplied::Updated => self
                .projection
                .rederive_message(&self.mechanism, message_id),
        }
        self.projection.mark_message_dirty(message_id);
    }

    /// Read a message's optimistic projection (None if not held or destroyed).
    pub fn message(&self, message_id: &str) -> Option<Value> {
        self.mechanism.optimistic_projection(message_id)
    }

    /// Read a view's rows.
    pub fn view_rows(&self, view_id: &str) -> Option<&[ViewRow]> {
        self.projection.view_rows(view_id)
    }

    /// Drain the keys changed since the last drain. The host re-reads these.
    pub fn drain_dirty(&mut self) -> Vec<DirtyKey> {
        self.projection.drain_dirty()
    }

    /// Drain the ids of ops retired since the last drain (at settle confirm or
    /// at base catch-up). The host clears the corresponding durable-outbox
    /// records only for these — an un-retired op is still pending in-engine and
    /// must survive a reload to be replayed. (outbox D)
    pub fn drain_retired(&mut self) -> Vec<MutationId> {
        self.mechanism.drain_retired()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_replica_core::{MessageAssertion, MutationId, SettlementOutcome};
    use serde_json::json;

    fn summary(id: &str, received_at: &str, mailboxes: &[&str]) -> Value {
        json!({
            "id": id,
            "sourceId": "primary",
            "receivedAt": received_at,
            "mailboxIds": mailboxes,
            "keywords": [],
            "isRead": false,
            "isFlagged": false,
            "subject": id,
        })
    }

    fn key(received_at: &str, id: &str) -> SortKey {
        SortKey {
            received_at: received_at.into(),
            message_id: id.into(),
        }
    }

    fn inbox_view() -> (EntityStore, &'static str) {
        let mut store = EntityStore::new();
        // A small window: m2 held at the watermark, with more below (watermark Some).
        store.register_view(
            "inbox",
            ViewPredicate::InMailboxes(vec!["inbox".into()]),
            "receivedAt".into(),
            SortDirection::Desc,
            Some(key("2026-04-28T12:00:00Z", "m2")),
        );
        store.set_view_rows(
            "inbox",
            vec![ViewRow {
                row_key: "primary:m2".into(),
                message_id: "m2".into(),
                sort_key: key("2026-04-28T12:00:00Z", "m2"),
            }],
            Some(key("2026-04-28T12:00:00Z", "m2")),
        );
        store.drain_dirty();
        (store, "inbox")
    }

    /// Ingest m2 so it has a confirmed base to fold over (set_view_rows places
    /// the row but does not seed the base; the projection arrives via ingest).
    fn ingest_m2(store: &mut EntityStore, keywords: &[&str]) {
        let mut proj = summary("m2", "2026-04-28T12:00:00Z", &["inbox"]);
        proj["keywords"] = json!(keywords);
        proj["isRead"] = json!(keywords.contains(&"$seen"));
        proj["isFlagged"] = json!(keywords.contains(&"$flagged"));
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m2".into(),
            projection: proj,
            deleted: false,
        }]);
    }

    /// Ingest m2 stamped with an authority-state `version` (for the staleness guard).
    fn ingest_m2_v(store: &mut EntityStore, keywords: &[&str], version: u64) {
        let mut proj = summary("m2", "2026-04-28T12:00:00Z", &["inbox"]);
        proj["keywords"] = json!(keywords);
        proj["isRead"] = json!(keywords.contains(&"$seen"));
        proj["isFlagged"] = json!(keywords.contains(&"$flagged"));
        proj["version"] = json!(version);
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m2".into(),
            projection: proj,
            deleted: false,
        }]);
    }

    /// Ingest m2 in `mailboxes` stamped with an authority `version` (the move
    /// flicker: a local move does not bump modseq, so the moved base and a stale
    /// re-serve share the version).
    fn ingest_m2_mailbox_v(store: &mut EntityStore, mailboxes: &[&str], version: u64) {
        let mut proj = summary("m2", "2026-04-28T12:00:00Z", mailboxes);
        proj["version"] = json!(version);
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m2".into(),
            projection: proj,
            deleted: false,
        }]);
    }

    fn flag_assertion() -> MessageAssertion {
        MessageAssertion::SetKeywords {
            add: vec!["$flagged".into()],
            remove: vec![],
        }
    }

    // --- authoritative placement (unchanged behavior) ----------------------

    #[test]
    fn in_range_arrival_is_placed_at_top_of_desc_view() {
        let (mut store, view) = inbox_view();
        // m1 is newer than the watermark m2 → sorts above it, in range.
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m1".into(),
            projection: summary("m1", "2026-04-29T10:00:00Z", &["inbox"]),
            deleted: false,
        }]);
        let ids: Vec<&str> = store
            .view_rows(view)
            .unwrap()
            .iter()
            .map(|r| r.message_id.as_str())
            .collect();
        assert_eq!(ids, vec!["m1", "m2"]);
    }

    #[test]
    fn below_watermark_arrival_is_ignored() {
        let (mut store, view) = inbox_view();
        // m3 is older than the watermark → out of range, must not be placed.
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m3".into(),
            projection: summary("m3", "2026-04-27T10:00:00Z", &["inbox"]),
            deleted: false,
        }]);
        let ids: Vec<&str> = store
            .view_rows(view)
            .unwrap()
            .iter()
            .map(|r| r.message_id.as_str())
            .collect();
        assert_eq!(ids, vec!["m2"]);
        // But the message entity itself is stored (discovery happened).
        assert!(store.message("m3").is_some());
    }

    #[test]
    fn asc_view_honours_its_watermark_direction() {
        // An Asc view holds `[TOP, W]` with W at the BOTTOM of the window: in
        // range means at-or-below W (older). m2 is the watermark.
        let mut store = EntityStore::new();
        store.register_view(
            "asc",
            ViewPredicate::InMailboxes(vec!["inbox".into()]),
            "receivedAt".into(),
            SortDirection::Asc,
            Some(key("2026-04-28T12:00:00Z", "m2")),
        );
        store.set_view_rows(
            "asc",
            vec![ViewRow {
                row_key: "primary:m2".into(),
                message_id: "m2".into(),
                sort_key: key("2026-04-28T12:00:00Z", "m2"),
            }],
            Some(key("2026-04-28T12:00:00Z", "m2")),
        );
        store.drain_dirty();

        // m1 is newer (above the watermark) → out of range for Asc, must be
        // ignored. The buggy Desc-only comparison would place it.
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m1".into(),
            projection: summary("m1", "2026-04-29T10:00:00Z", &["inbox"]),
            deleted: false,
        }]);
        // m3 is older (at-or-below the watermark) → in range, placed before m2
        // in ascending order. The buggy comparison would drop it.
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m3".into(),
            projection: summary("m3", "2026-04-27T10:00:00Z", &["inbox"]),
            deleted: false,
        }]);

        let ids: Vec<&str> = store
            .view_rows("asc")
            .unwrap()
            .iter()
            .map(|r| r.message_id.as_str())
            .collect();
        assert_eq!(ids, vec!["m3", "m2"]);
    }

    #[test]
    fn membership_loss_removes_a_held_row() {
        let (mut store, view) = inbox_view();
        // m2 leaves the inbox mailbox → removed from the view; coverage intact.
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m2".into(),
            projection: summary("m2", "2026-04-28T12:00:00Z", &["archive"]),
            deleted: false,
        }]);
        assert!(store.view_rows(view).unwrap().is_empty());
    }

    #[test]
    fn deletion_removes_a_held_row() {
        let (mut store, view) = inbox_view();
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m2".into(),
            projection: summary("m2", "2026-04-28T12:00:00Z", &["inbox"]),
            deleted: true,
        }]);
        assert!(store.view_rows(view).unwrap().is_empty());
        assert!(store.message("m2").is_none());
    }

    #[test]
    fn batch_is_atomic_one_dirty_drain() {
        let (mut store, view) = inbox_view();
        // Two arrivals in one batch → the view is dirty once, not twice.
        store.ingest_batch(vec![
            StoreUpdate::Message {
                message_id: "m1".into(),
                projection: summary("m1", "2026-04-29T10:00:00Z", &["inbox"]),
                deleted: false,
            },
            StoreUpdate::Message {
                message_id: "m0".into(),
                projection: summary("m0", "2026-04-30T10:00:00Z", &["inbox"]),
                deleted: false,
            },
        ]);
        let dirty = store.drain_dirty();
        let view_dirty_count = dirty
            .iter()
            .filter(|k| matches!(k, DirtyKey::View(v) if v == view))
            .count();
        assert_eq!(view_dirty_count, 1, "a batch notifies a view once");
        let ids: Vec<&str> = store
            .view_rows(view)
            .unwrap()
            .iter()
            .map(|r| r.message_id.as_str())
            .collect();
        assert_eq!(ids, vec!["m0", "m1", "m2"]);
    }

    #[test]
    fn complete_view_has_no_watermark_so_everything_is_in_range() {
        let mut store = EntityStore::new();
        store.register_view(
            "all",
            ViewPredicate::All,
            "receivedAt".into(),
            SortDirection::Desc,
            None, // reaches BOTTOM
        );
        store.drain_dirty();
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m9".into(),
            projection: summary("m9", "2020-01-01T00:00:00Z", &["inbox"]),
            deleted: false,
        }]);
        assert_eq!(store.view_rows("all").unwrap().len(), 1);
    }

    #[test]
    fn deferred_view_is_not_self_maintained_by_mutations() {
        let mut store = EntityStore::new();
        store.register_view(
            "search",
            ViewPredicate::Deferred,
            "receivedAt".into(),
            SortDirection::Desc,
            None,
        );
        store.drain_dirty();
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m1".into(),
            projection: summary("m1", "2026-04-29T10:00:00Z", &["inbox"]),
            deleted: false,
        }]);
        // The host drives a deferred view via set_view_rows; a raw mutation
        // does not place a row.
        assert!(store.view_rows("search").unwrap().is_empty());
    }

    #[test]
    fn in_mailboxes_places_a_row_matching_any_set_member() {
        // "All Inboxes": the inbox-role mailbox in two accounts. A message in
        // either account's inbox is placed; one in neither is ignored.
        let mut store = EntityStore::new();
        store.register_view(
            "all-inboxes",
            ViewPredicate::InMailboxes(vec!["inbox-a".into(), "inbox-b".into()]),
            "receivedAt".into(),
            SortDirection::Desc,
            None,
        );
        store.drain_dirty();
        store.ingest_batch(vec![
            StoreUpdate::Message {
                message_id: "m-a".into(),
                projection: summary("m-a", "2026-04-29T12:00:00Z", &["inbox-b"]),
                deleted: false,
            },
            StoreUpdate::Message {
                message_id: "m-c".into(),
                projection: summary("m-c", "2026-04-29T11:00:00Z", &["archive-a"]),
                deleted: false,
            },
        ]);
        let rows = store.view_rows("all-inboxes").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].message_id, "m-a");
    }

    // --- optimism -----------------------------------------------------------

    #[test]
    fn optimistic_flag_shows_in_message_and_view_before_confirm() {
        let (mut store, view) = inbox_view();
        ingest_m2(&mut store, &[]);
        store.drain_dirty();

        store.accept_mutation(MutationId("op1".into()), "m2", flag_assertion());

        // The message projection reflects the optimistic flag.
        let proj = store.message("m2").unwrap();
        assert_eq!(proj["isFlagged"], json!(true));
        assert_eq!(proj["keywords"], json!(["$flagged"]));
        // The view row still holds m2 (a flag does not change membership).
        let ids: Vec<&str> = store
            .view_rows(view)
            .unwrap()
            .iter()
            .map(|r| r.message_id.as_str())
            .collect();
        assert_eq!(ids, vec!["m2"]);
        assert!(store.has_pending());
        let dirty = store.drain_dirty();
        assert!(dirty.contains(&DirtyKey::Message("m2".into())));
    }

    // --- reverse index + content-only dirty-marking -------------------------

    /// The reverse index a message is sorted member-list for, for assertions.
    fn views_of<'a>(store: &'a EntityStore, message_id: &str) -> Vec<&'a str> {
        let mut views: Vec<&str> = store
            .projection
            .message_views
            .get(message_id)
            .map(|set| set.iter().map(String::as_str).collect())
            .unwrap_or_default();
        views.sort_unstable();
        views
    }

    #[test]
    fn content_only_rederive_marks_the_owning_view_dirty() {
        // The bug: a flag/read toggle leaves the sort key (and so the row tuple)
        // unchanged, so the old `changed` gate never marked the view dirty —
        // the host then had to re-project every view to catch it.
        let (mut store, view) = inbox_view();
        ingest_m2(&mut store, &[]);
        store.drain_dirty();
        assert_eq!(views_of(&store, "m2"), vec![view], "m2 indexed in the view");

        // A content-only optimism: same sort key, new keywords.
        store.accept_mutation(MutationId("op1".into()), "m2", flag_assertion());

        let dirty = store.drain_dirty();
        assert!(
            dirty.contains(&DirtyKey::View(view.into())),
            "a content-only rederive must mark the owning view dirty: {dirty:?}"
        );
        // The row is still held (a flag does not change membership) and indexed.
        assert_eq!(views_of(&store, "m2"), vec![view]);
    }

    #[test]
    fn content_only_change_to_a_message_in_no_view_marks_nothing() {
        let (mut store, _view) = inbox_view();
        // m3 sits below the watermark → ingested but placed in no view.
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m3".into(),
            projection: summary("m3", "2026-04-27T10:00:00Z", &["inbox"]),
            deleted: false,
        }]);
        store.drain_dirty();
        assert!(views_of(&store, "m3").is_empty(), "m3 is in no view");

        store.accept_mutation(MutationId("op1".into()), "m3", flag_assertion());

        let dirty = store.drain_dirty();
        assert!(
            !dirty.iter().any(|k| matches!(k, DirtyKey::View(_))),
            "a change to a message in no view must mark no view dirty: {dirty:?}"
        );
        // The message itself is still reported dirty (its projection moved).
        assert!(dirty.contains(&DirtyKey::Message("m3".into())));
    }

    #[test]
    fn reverse_index_tracks_insert_membership_loss_and_deletion() {
        let (mut store, view) = inbox_view();
        // Arrival in range → placed → indexed.
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m1".into(),
            projection: summary("m1", "2026-04-29T10:00:00Z", &["inbox"]),
            deleted: false,
        }]);
        assert_eq!(views_of(&store, "m1"), vec![view]);

        // Membership loss (m1 leaves the inbox mailbox) → row + index drop.
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m1".into(),
            projection: summary("m1", "2026-04-29T10:00:00Z", &["archive"]),
            deleted: false,
        }]);
        assert!(
            views_of(&store, "m1").is_empty(),
            "moved-out row de-indexed"
        );

        // Re-arrival then authoritative deletion → index entry purged entirely.
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m1".into(),
            projection: summary("m1", "2026-04-29T10:00:00Z", &["inbox"]),
            deleted: false,
        }]);
        assert_eq!(views_of(&store, "m1"), vec![view]);
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m1".into(),
            projection: summary("m1", "2026-04-29T10:00:00Z", &["inbox"]),
            deleted: true,
        }]);
        assert!(views_of(&store, "m1").is_empty(), "deleted row de-indexed");
    }

    #[test]
    fn set_view_rows_and_close_view_keep_the_index_correct() {
        let (mut store, view) = inbox_view();
        // inbox_view seeds m2 via set_view_rows → indexed.
        assert_eq!(views_of(&store, "m2"), vec![view]);

        // Re-serve a snapshot that replaces m2 with m9 → index follows the swap.
        store.set_view_rows(
            view,
            vec![ViewRow {
                row_key: "primary:m9".into(),
                message_id: "m9".into(),
                sort_key: key("2026-04-28T13:00:00Z", "m9"),
            }],
            Some(key("2026-04-28T13:00:00Z", "m9")),
        );
        assert!(views_of(&store, "m2").is_empty(), "replaced row de-indexed");
        assert_eq!(views_of(&store, "m9"), vec![view]);

        // Closing the view drops its membership from the index.
        store.close_view(view);
        assert!(views_of(&store, "m9").is_empty(), "closed view de-indexed");
    }

    #[test]
    fn authoritative_delete_purges_pending_op() {
        let (mut store, _view) = inbox_view();
        ingest_m2(&mut store, &[]);
        store.drain_dirty();

        // A pending optimistic op on m2 (a flag).
        store.accept_mutation(MutationId("op1".into()), "m2", flag_assertion());
        assert!(
            store.has_pending(),
            "op pending before the authoritative delete"
        );

        // Authoritative removal of m2 (expunge / rule / another client): a
        // `message.updated` with `deleted: true` and no projection.
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m2".into(),
            projection: json!(null),
            deleted: true,
        }]);

        // The op is purged — it can neither fold into nor revert against a
        // deleted entity, so it must not leak pending forever (`has_pending`
        // stuck true; the durable outbox growing unbounded on a delete-heavy
        // workload). Before the fix the version-gated retire on
        // `settle(Confirmed)` couldn't reach a deleted entity (no version), and
        // unconfirmed ops are never retired there anyway.
        assert!(
            !store.has_pending(),
            "pending op purged on authoritative delete"
        );
        assert!(store.mechanism.engine.pending().is_empty());

        // A late `settle(Confirmed)` for the now-purged op is a no-op (the op is
        // absent from pending -> settle finds no key -> no retire), not a leak.
        let result = store.settle(&MutationId("op1".into()), SettlementOutcome::Confirmed);
        assert!(!result.retired, "late confirm on the purged op is a no-op");
        assert!(!store.has_pending());
    }

    #[test]
    fn optimistic_archive_drops_row_but_keeps_message() {
        let (mut store, view) = inbox_view();
        ingest_m2(&mut store, &[]);
        store.drain_dirty();

        store.accept_mutation(
            MutationId("op1".into()),
            "m2",
            MessageAssertion::ReplaceMailboxes {
                mailbox_ids: vec!["archive".into()],
            },
        );

        // The inbox view drops m2 (membership moved to archive)...
        assert!(store.view_rows(view).unwrap().is_empty());
        // ...but the message is still held (base intact; only the fold moved it).
        let proj = store.message("m2").unwrap();
        assert_eq!(proj["mailboxIds"], json!(["archive"]));
        assert!(store.has_pending());
    }

    #[test]
    fn move_op_holds_through_equal_version_then_retires_on_bump() {
        // The .20 flicker root cause: a LOCAL move does not bump the provider
        // modseq, so the moved [archive]@v5 base and a stale [inbox]@v5 re-serve
        // are EQUAL version. The op must NOT retire on the equal-version echo +
        // confirm (5 == 5) — else the stale re-serve clobbers membership with no
        // op to re-fold [archive] over it. It holds through the window, retiring
        // only on the real modseq+1 bump.
        let (mut store, view) = inbox_view();
        ingest_m2_v(&mut store, &[], 5); // m2 in inbox @ v5
        store.drain_dirty();
        assert_eq!(store.view_rows(view).unwrap().len(), 1);

        // Local move to archive (optimism folds archive over the inbox@v5 base).
        store.accept_mutation(
            MutationId("op1".into()),
            "m2",
            MessageAssertion::ReplaceMailboxes {
                mailbox_ids: vec!["archive".into()],
            },
        );
        assert!(store.view_rows(view).unwrap().is_empty()); // m2 leaves inbox

        // Provider's same-modseq [archive]@v5 (move applied, modseq not yet
        // bumped) + the verdict Confirmed: the op must NOT retire at 5 == 5.
        ingest_m2_mailbox_v(&mut store, &["archive"], 5);
        let result = store.settle(&MutationId("op1".into()), SettlementOutcome::Confirmed);
        assert!(!result.retired, "op must hold at equal version");
        assert!(store.has_pending());
        assert!(store.view_rows(view).unwrap().is_empty());

        // A STALE [inbox]@v5 re-serve (equal version) clobbers the base — but the
        // op is still pending, folding [archive] over it, so m2 stays out.
        ingest_m2_mailbox_v(&mut store, &["inbox"], 5);
        assert!(
            store.view_rows(view).unwrap().is_empty(),
            "stale equal-version re-serve must not re-add the row"
        );
        assert!(
            store.has_pending(),
            "op still holds through the stale re-serve"
        );

        // Provider confirms with modseq+1 ([archive]@v6): strictly higher → retire.
        ingest_m2_mailbox_v(&mut store, &["archive"], 6);
        assert!(!store.has_pending(), "op retires on the real modseq bump");
        assert!(store.view_rows(view).unwrap().is_empty());
        assert_eq!(
            store.message("m2").unwrap()["mailboxIds"],
            json!(["archive"])
        );
    }

    #[test]
    fn unconfirmed_op_survives_a_base_update_that_carries_it() {
        // The Bug-1 fix at the store level: a base update that carries the effect
        // (a local message.updated echo, or a stale provider re-serve) must NOT
        // retire an op the authority has not yet confirmed — it stays folded
        // (idempotent, invisible), so a later stale re-serve cannot revert it.
        // Retirement waits for the keyed confirmation.
        let (mut store, _view) = inbox_view();
        ingest_m2(&mut store, &[]);
        store.drain_dirty();

        store.accept_mutation(MutationId("op1".into()), "m2", flag_assertion());
        // A base update carrying the flag arrives (the optimistic echo): the op
        // stays pending, the projection stays flagged — no early retire.
        ingest_m2(&mut store, &["$flagged"]);
        assert!(store.has_pending());
        assert_eq!(store.message("m2").unwrap()["isFlagged"], json!(true));

        // Only the authority's confirmation retires it (the base already carries
        // the effect, so this is a no-op visually — still flagged, no revert).
        let result = store.settle(&MutationId("op1".into()), SettlementOutcome::Confirmed);
        assert!(result.retired);
        assert!(!result.reverted);
        assert!(!store.has_pending());
        assert_eq!(store.message("m2").unwrap()["isFlagged"], json!(true));
    }

    #[test]
    fn confirm_before_base_update_does_not_revert() {
        // The regression test for the flicker: a confirmation that outruns the
        // authoritative `message.updated` must NOT flip the projection back to
        // the stale base. The op stays folded until the base catches up.
        let (mut store, _view) = inbox_view();
        ingest_m2(&mut store, &[]);
        store.drain_dirty();

        store.accept_mutation(MutationId("op1".into()), "m2", flag_assertion());
        assert_eq!(store.message("m2").unwrap()["isFlagged"], json!(true));

        // Confirmation arrives FIRST (base has not caught up). No revert.
        let result = store.settle(&MutationId("op1".into()), SettlementOutcome::Confirmed);
        assert!(!result.retired);
        assert!(!result.reverted);
        assert!(store.has_pending());
        assert_eq!(store.message("m2").unwrap()["isFlagged"], json!(true));

        // Then the base update lands and retires the op — still flagged, still no
        // revert anywhere in the sequence.
        ingest_m2(&mut store, &["$flagged"]);
        assert!(!store.has_pending());
        assert_eq!(store.message("m2").unwrap()["isFlagged"], json!(true));
    }

    #[test]
    fn stale_re_serve_does_not_re_add_a_moved_out_row() {
        // The move/archive/delete flicker: a row leaves the inbox, then a stale
        // view re-serve that still lists it must NOT re-add it (it must not
        // "come back and stay until refresh"). set_view_rows reconciles against
        // the version-guarded base, not the served list.
        let (mut store, view) = inbox_view();
        ingest_m2_v(&mut store, &[], 1); // m2 in inbox @ v1
        store.drain_dirty();
        assert_eq!(store.view_rows(view).unwrap().len(), 1);

        // The move: m2 leaves inbox (authoritative, no client optimism) @ v2.
        let mut moved = summary("m2", "2026-04-28T12:00:00Z", &["archive"]);
        moved["version"] = json!(2);
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m2".into(),
            projection: moved,
            deleted: false,
        }]);
        assert!(store.view_rows(view).unwrap().is_empty()); // the blink: m2 gone

        // A STALE re-serve still lists m2 in inbox @ v1: the version guard rejects
        // the base (1 < 2), and set_view_rows must reconcile m2 away.
        ingest_m2_v(&mut store, &[], 1);
        store.set_view_rows(
            view,
            vec![ViewRow {
                row_key: "primary:m2".into(),
                message_id: "m2".into(),
                sort_key: key("2026-04-28T12:00:00Z", "m2"),
            }],
            Some(key("2026-04-28T12:00:00Z", "m2")),
        );

        // m2 stays gone, and the held base is still archive (guard held).
        assert!(store.view_rows(view).unwrap().is_empty());
        assert_eq!(
            store.message("m2").unwrap()["mailboxIds"],
            json!(["archive"])
        );
    }

    #[test]
    fn ingest_rejects_a_strictly_older_authority_version() {
        // The Bug-1b tail: after the op has legitimately retired (confirmed +
        // absorbed), a late stale provider re-serve carrying an OLDER authority
        // version must be rejected, so it cannot clobber the newer confirmed base.
        let (mut store, _view) = inbox_view();
        ingest_m2_v(&mut store, &[], 1);
        store.drain_dirty();

        store.accept_mutation(MutationId("op1".into()), "m2", flag_assertion());
        // Provider applies the flag @ v2; confirm it (op retires).
        ingest_m2_v(&mut store, &["$flagged"], 2);
        store.settle(&MutationId("op1".into()), SettlementOutcome::Confirmed);
        assert!(!store.has_pending());
        assert_eq!(store.message("m2").unwrap()["isFlagged"], json!(true));

        // A late STALE re-serve @ v1 (1 < 2) is rejected — the flag holds.
        ingest_m2_v(&mut store, &[], 1);
        assert_eq!(store.message("m2").unwrap()["isFlagged"], json!(true));

        // A genuinely newer state @ v3 (a real unflag) is accepted.
        ingest_m2_v(&mut store, &[], 3);
        assert_eq!(store.message("m2").unwrap()["isFlagged"], json!(false));
    }

    #[test]
    fn confirm_retires_a_no_op_optimism_without_any_base_update() {
        // Flagging an already-flagged message: the optimism is absorbed from the
        // start, so no `message.updated` will carry it. The confirmation clears
        // the otherwise-lingering op.
        let (mut store, _view) = inbox_view();
        ingest_m2(&mut store, &["$flagged"]);
        store.drain_dirty();

        store.accept_mutation(MutationId("op1".into()), "m2", flag_assertion());
        assert!(store.has_pending());

        let result = store.settle(&MutationId("op1".into()), SettlementOutcome::Confirmed);
        assert!(result.retired);
        assert!(!result.reverted);
        assert!(!store.has_pending());
        assert_eq!(store.message("m2").unwrap()["isFlagged"], json!(true));
    }

    #[test]
    fn failed_settle_reverts_the_optimistic_change() {
        let (mut store, _view) = inbox_view();
        ingest_m2(&mut store, &[]);
        store.drain_dirty();

        store.accept_mutation(MutationId("op1".into()), "m2", flag_assertion());
        assert_eq!(store.message("m2").unwrap()["isFlagged"], json!(true));

        let result = store.settle(&MutationId("op1".into()), SettlementOutcome::Failed);
        assert!(result.reverted);
        assert!(!store.has_pending());
        assert_eq!(store.message("m2").unwrap()["isFlagged"], json!(false));
    }

    #[test]
    fn pending_optimism_survives_an_unrelated_base_update() {
        let (mut store, view) = inbox_view();
        ingest_m2(&mut store, &[]);
        store.drain_dirty();

        store.accept_mutation(MutationId("op1".into()), "m2", flag_assertion());
        // A sibling arrival (m0, newer) re-serves its own base WITHOUT the flag.
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m0".into(),
            projection: summary("m0", "2026-04-30T10:00:00Z", &["inbox"]),
            deleted: false,
        }]);

        // m2's optimism survived the rebase (its pending was not cleared).
        assert_eq!(store.message("m2").unwrap()["isFlagged"], json!(true));
        assert!(store.has_pending());
        let ids: Vec<&str> = store
            .view_rows(view)
            .unwrap()
            .iter()
            .map(|r| r.message_id.as_str())
            .collect();
        assert_eq!(ids, vec!["m0", "m2"]);
    }

    #[test]
    fn optimistic_destroy_drops_row_and_reverts_on_failure() {
        let (mut store, view) = inbox_view();
        ingest_m2(&mut store, &[]);
        store.drain_dirty();

        store.accept_mutation(MutationId("op1".into()), "m2", MessageAssertion::Destroy);
        // Optimistically destroyed: row gone, message reads None.
        assert!(store.view_rows(view).unwrap().is_empty());
        assert!(store.message("m2").is_none());
        assert!(store.has_pending());

        // A failed destroy reverts: the row returns.
        store.settle(&MutationId("op1".into()), SettlementOutcome::Failed);
        assert!(store.message("m2").is_some());
        assert_eq!(store.view_rows(view).unwrap().len(), 1);
        assert!(!store.has_pending());
    }

    #[test]
    fn accept_on_uningested_message_is_deferred_not_dropped() {
        // inbox_view places m2 via set_view_rows but never ingests its
        // projection, so there is no base to fold over. The mutation is tracked
        // (pending) and folds in once the base arrives — it does not remove the
        // authoritative row.
        let (mut store, view) = inbox_view();
        store.drain_dirty();

        store.accept_mutation(MutationId("op1".into()), "m2", flag_assertion());
        assert!(store.has_pending());
        assert_eq!(
            store.view_rows(view).unwrap().len(),
            1,
            "row left untouched"
        );

        // The base arrives: optimism folds in.
        ingest_m2(&mut store, &[]);
        assert_eq!(store.message("m2").unwrap()["isFlagged"], json!(true));
    }

    // --- WASM boundary serde shapes ----------------------------------------
    // The web adapter builds these JSON shapes; the WASM handle deserializes
    // them into the store types. Pin the camelCase wire contract here so a
    // Rust rename can't silently break the host.

    #[test]
    fn store_update_message_round_trips_camel_case() {
        let update = StoreUpdate::Message {
            message_id: "m1".into(),
            projection: summary("m1", "2026-04-29T10:00:00Z", &["inbox"]),
            deleted: false,
        };
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(
            json["message"]["messageId"],
            json!("m1"),
            "message update is externally-tagged + camelCase"
        );
        assert_eq!(json["message"]["deleted"], json!(false));
        // The countDelta channel is deleted (RFC-L2-count-unification): a
        // message update carries no counts on the wire.
        assert!(json["message"].get("countDeltas").is_none());
        // Round-trips back unchanged.
        let back: StoreUpdate = serde_json::from_value(json).unwrap();
        assert_eq!(back, update);
    }

    #[test]
    fn store_update_tolerates_a_legacy_count_deltas_field() {
        // A stale producer (an old runtime still attaching countDeltas) must not
        // brick ingestion: unknown fields are ignored by serde's default.
        let json = json!({
            "message": {
                "messageId": "m1",
                "projection": summary("m1", "2026-04-29T10:00:00Z", &["inbox"]),
                "deleted": false,
                "countDeltas": [
                    {"mailboxId": "inbox", "unreadCount": 3, "totalCount": 10}
                ]
            }
        });
        let back: StoreUpdate = serde_json::from_value(json).unwrap();
        let StoreUpdate::Message { message_id, .. } = back;
        assert_eq!(message_id, "m1");
    }

    #[test]
    fn dirty_key_serializes_externally_tagged() {
        // The host drains dirty as a JSON array of these.
        assert_eq!(
            serde_json::to_value(DirtyKey::Message("m1".into())).unwrap(),
            json!({"message": "m1"})
        );
        assert_eq!(
            serde_json::to_value(DirtyKey::View("inbox".into())).unwrap(),
            json!({"view": "inbox"})
        );
    }

    #[test]
    fn view_row_and_predicate_round_trip_camel_case() {
        let row = ViewRow {
            row_key: "primary:m1".into(),
            message_id: "m1".into(),
            sort_key: SortKey {
                received_at: "2026-04-29T10:00:00Z".into(),
                message_id: "m1".into(),
            },
        };
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(json["rowKey"], json!("primary:m1"));
        assert_eq!(json["sortKey"]["receivedAt"], json!("2026-04-29T10:00:00Z"));
        let back: ViewRow = serde_json::from_value(json).unwrap();
        assert_eq!(back, row);

        assert_eq!(
            serde_json::to_value(ViewPredicate::InMailboxes(vec!["inbox".into()])).unwrap(),
            json!({"inMailboxes": ["inbox"]})
        );
        assert_eq!(
            serde_json::to_value(ViewPredicate::All).unwrap(),
            json!("all")
        );
        assert_eq!(
            serde_json::to_value(ViewPredicate::Deferred).unwrap(),
            json!("deferred")
        );
    }
}
