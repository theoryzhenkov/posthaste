//! The reactive entity store core (merged slices 2+3, sub-slice 2a).
//!
//! Generalizes [`crate::MailListReplica`] from a single mail-list view into a
//! normalized, keyed entity store with register-by-use / drain-dirty reactivity:
//! `message[id]`, `mailbox[id]` (server-authoritative count scalars), and
//! `view[viewId]` (an ordered row list plus coverage). The host feeds it
//! authoritative batches — message mutations (carrying the full `MessageSummary`
//! projection, per `firehose-carries-rows`) and count deltas — and the store
//! applies the whole batch atomically, then reports the keys that changed via
//! [`EntityStore::drain_dirty`]. The host does the reactive fan-out (write the
//! changed keys into the renderer cache); the store is a dumb dirty-tracker.
//!
//! For an **evaluable** predicate (`InMailbox`, `All`) the store self-maintains
//! each view's membership: on a message mutation it runs one local evaluation —
//! place the row if the predicate matches *and* the message's sort key is within
//! the held coverage `[TOP, W]`, otherwise ignore (or remove, if it was held and
//! no longer matches). A mutation can shrink or hold the range, never grow it
//! downward; only paging grows `W` (`mutations-absorb-or-ignore`,
//! `paging-grows-range`). A **deferred** predicate is never self-evaluated; the
//! host drives its membership via deltas.
//!
//! This sub-slice is authoritative-only: it does not yet fold the outbox
//! (`posthaste-link-core`'s `MessageReplica`) — that layers on a following
//! sub-slice over the same entity model. Coverage is held as the watermark `W`
//! (the sort key of the last held row; `None` = reaches BOTTOM); the full
//! multi-range `CoverageRange` shape is adopted when jump-to-date lands. The
//! typed `SortKey` supports the `[receivedAt, id]` composite (the default and
//! overwhelmingly common sort); views under a different sort are `Deferred`.
//!
//! @spec docs/eph/DESIGN-L2-client-link-reactive-store
//! @spec docs/eph/PLAN-L2-client-link-reactive-store

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A changed entity key, reported by [`EntityStore::drain_dirty`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DirtyKey {
    Message(String),
    Mailbox(String),
    View(String),
}

/// Sort direction of a view's order; selects the in-range comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortDirection {
    Asc,
    Desc,
}

/// The composite sort key `[receivedAt, id]` the store places rows by. A typed
/// pair (rather than a raw `serde_json::Value`) so ordering is well-defined and
/// cheap; it matches the runtime's `mail_list_state` sort key. Lexicographic on
/// `(received_at, message_id)` — ISO-8601 timestamps compare chronologically
/// and the id is a stable tiebreak.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SortKey {
    pub received_at: String,
    pub message_id: String,
}

/// A view's membership predicate. `InMailbox`/`All` are **evaluable** (the store
/// self-maintains placement); `Deferred` is not (the host drives deltas).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewPredicate {
    InMailbox(String),
    All,
    Deferred,
}

impl ViewPredicate {
    /// Whether a message matches this view's filter. `Deferred` returns `true`
    /// (the store does not decide; the host places via deltas).
    fn matches(&self, projection: &Value) -> bool {
        match self {
            ViewPredicate::InMailbox(mailbox_id) => projection
                .get("mailboxIds")
                .and_then(Value::as_array)
                .map(|ids| ids.iter().any(|id| id.as_str() == Some(mailbox_id)))
                .unwrap_or(false),
            ViewPredicate::All => true,
            ViewPredicate::Deferred => true,
        }
    }

    fn is_evaluable(&self) -> bool {
        !matches!(self, ViewPredicate::Deferred)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewRow {
    pub row_key: String,
    pub message_id: String,
    /// The composite sort key the row was placed at, held so a later mutation
    /// can insert/move against it without re-reading the projection; refreshed
    /// from the authority on `set_view_rows`.
    pub sort_key: SortKey,
}

/// A view entity: an ordered row list plus the coverage watermark and the
/// predicate/sort that govern local placement.
#[derive(Clone, Debug)]
pub struct ViewEntity {
    pub predicate: ViewPredicate,
    pub sort_field: String,
    pub sort_direction: SortDirection,
    /// The sort key of the last held row; `None` = the range reaches BOTTOM
    /// (the view holds every match). The boundary a mutation cannot cross
    /// downward — only paging moves it toward BOTTOM.
    pub watermark: Option<SortKey>,
    pub rows: Vec<ViewRow>,
}

/// A mailbox entity: server-authoritative count scalars (the store is partial,
/// so counts are never derived from the held message set).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MailboxEntity {
    pub unread_count: i64,
    pub total_count: i64,
}

/// A message entity: the authoritative `MessageSummary` projection.
#[derive(Clone, Debug)]
pub struct MessageEntity {
    pub projection: Value,
}

/// A count delta shipped with a message event (atomic per batch — `D3`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CountDelta {
    pub mailbox_id: String,
    pub unread_count: i64,
    pub total_count: i64,
}

/// One authoritative update in a batch.
#[derive(Clone, Debug)]
pub enum StoreUpdate {
    /// A message mutation carrying the full projection (enough to evaluate
    /// membership, compute the sort key, and render) plus the affected
    /// mailboxes' new counts (`firehose-carries-rows`, `D2`).
    Message {
        message_id: String,
        projection: Value,
        deleted: bool,
        count_deltas: Vec<CountDelta>,
    },
    /// A standalone count delta (e.g. a mailbox metadata event).
    MailboxCount(CountDelta),
}

/// The reactive entity store. Pure compute: no transport, no persistence.
#[derive(Default)]
pub struct EntityStore {
    messages: HashMap<String, MessageEntity>,
    mailboxes: HashMap<String, MailboxEntity>,
    views: HashMap<String, ViewEntity>,
    dirty: HashSet<DirtyKey>,
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
        self.views.insert(
            view_id.to_string(),
            ViewEntity {
                predicate,
                sort_field,
                sort_direction,
                watermark,
                rows: Vec::new(),
            },
        );
        self.dirty.insert(DirtyKey::View(view_id.to_string()));
    }

    /// Replace a view's held rows and watermark (a served snapshot / page from
    /// the authority). Used on open, extend, and resync.
    pub fn set_view_rows(
        &mut self,
        view_id: &str,
        rows: Vec<ViewRow>,
        watermark: Option<SortKey>,
    ) {
        if let Some(view) = self.views.get_mut(view_id) {
            view.rows = rows;
            view.watermark = watermark;
            self.dirty.insert(DirtyKey::View(view_id.to_string()));
        }
    }

    pub fn close_view(&mut self, view_id: &str) {
        self.views.remove(view_id);
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
                    count_deltas,
                } => {
                    self.apply_message(&message_id, &projection, deleted);
                    for delta in count_deltas {
                        self.apply_count_delta(&delta);
                    }
                }
                StoreUpdate::MailboxCount(delta) => self.apply_count_delta(&delta),
            }
        }
    }

    fn apply_message(&mut self, message_id: &str, projection: &Value, deleted: bool) {
        if deleted {
            self.messages.remove(message_id);
        } else {
            self.messages.insert(
                message_id.to_string(),
                MessageEntity {
                    projection: projection.clone(),
                },
            );
        }
        self.dirty.insert(DirtyKey::Message(message_id.to_string()));

        // Place-or-ignore for every evaluable view. Deferred views are left to
        // the host's deltas. Snapshot the views (id + sort) so we can mutate
        // without borrowing the map under iteration.
        let sort_key = sort_key_of(projection, message_id);
        let views: Vec<(String, SortDirection, ViewPredicate, Option<SortKey>)> = self
            .views
            .iter()
            .map(|(id, v)| {
                (
                    id.clone(),
                    v.sort_direction,
                    v.predicate.clone(),
                    v.watermark.clone(),
                )
            })
            .collect();
        for (view_id, direction, predicate, watermark) in views {
            if !predicate.is_evaluable() {
                continue;
            }
            let view = self.views.get_mut(&view_id).expect("view present");
            let was_present = view.rows.iter().position(|r| r.message_id == message_id);
            let place =
                !deleted && predicate.matches(projection) && in_range(&sort_key, &watermark);
            let row = ViewRow {
                row_key: row_key_of(projection, message_id),
                message_id: message_id.to_string(),
                sort_key: sort_key.clone(),
            };
            let changed = match (place, was_present) {
                (true, Some(idx)) => {
                    if view.rows[idx] != row {
                        view.rows.remove(idx);
                        insert_sorted(&mut view.rows, row, direction);
                        true
                    } else {
                        false
                    }
                }
                (true, None) => {
                    insert_sorted(&mut view.rows, row, direction);
                    true
                }
                (false, Some(idx)) => {
                    view.rows.remove(idx);
                    true
                }
                (false, None) => false,
            };
            if changed {
                self.dirty.insert(DirtyKey::View(view_id));
            }
        }
    }

    fn apply_count_delta(&mut self, delta: &CountDelta) {
        let mailbox = self.mailboxes.entry(delta.mailbox_id.clone()).or_default();
        mailbox.unread_count = delta.unread_count;
        mailbox.total_count = delta.total_count;
        self.dirty.insert(DirtyKey::Mailbox(delta.mailbox_id.clone()));
    }

    /// Read a message projection (None if not held).
    pub fn message(&self, message_id: &str) -> Option<&Value> {
        self.messages.get(message_id).map(|m| &m.projection)
    }

    /// Read a mailbox's counts.
    pub fn mailbox(&self, mailbox_id: &str) -> Option<&MailboxEntity> {
        self.mailboxes.get(mailbox_id)
    }

    /// Read a view's rows.
    pub fn view_rows(&self, view_id: &str) -> Option<&[ViewRow]> {
        self.views.get(view_id).map(|v| v.rows.as_slice())
    }

    /// Drain the keys changed since the last drain. The host re-reads these.
    pub fn drain_dirty(&mut self) -> Vec<DirtyKey> {
        self.dirty.drain().collect()
    }
}

/// The composite sort key `[receivedAt, id]` read out of a projection.
fn sort_key_of(projection: &Value, message_id: &str) -> SortKey {
    SortKey {
        received_at: projection
            .get("receivedAt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        message_id: message_id.to_string(),
    }
}

fn row_key_of(projection: &Value, message_id: &str) -> String {
    let source_id = projection
        .get("sourceId")
        .and_then(Value::as_str)
        .unwrap_or("");
    format!("{source_id}:{message_id}")
}

/// Whether a sort key falls within a view's held range `[TOP, W]`. `None`
/// watermark means the range reaches BOTTOM (complete) — always in range.
/// Desc: at or above `W` (`sort_key >= W`); Asc: at or below `W` (`sort_key <= W`).
fn in_range(sort_key: &SortKey, watermark: &Option<SortKey>) -> bool {
    match watermark {
        None => true,
        Some(w) => match sort_key.cmp(w) {
            Ordering::Greater | Ordering::Equal => true,
            Ordering::Less => false,
        },
    }
}

/// Insert a row at its sorted position (stable for equal keys).
fn insert_sorted(rows: &mut Vec<ViewRow>, row: ViewRow, direction: SortDirection) {
    let pos = match direction {
        SortDirection::Desc => rows.iter().position(|r| r.sort_key < row.sort_key),
        SortDirection::Asc => rows.iter().position(|r| r.sort_key > row.sort_key),
    };
    rows.insert(pos.unwrap_or(rows.len()), row);
}

#[cfg(test)]
mod tests {
    use super::*;
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
            ViewPredicate::InMailbox("inbox".into()),
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

    #[test]
    fn in_range_arrival_is_placed_at_top_of_desc_view() {
        let (mut store, view) = inbox_view();
        // m1 is newer than the watermark m2 → sorts above it, in range.
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m1".into(),
            projection: summary("m1", "2026-04-29T10:00:00Z", &["inbox"]),
            deleted: false,
            count_deltas: vec![],
        }]);
        let ids: Vec<&str> =
            store.view_rows(view).unwrap().iter().map(|r| r.message_id.as_str()).collect();
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
            count_deltas: vec![],
        }]);
        let ids: Vec<&str> =
            store.view_rows(view).unwrap().iter().map(|r| r.message_id.as_str()).collect();
        assert_eq!(ids, vec!["m2"]);
        // But the message entity itself is stored (discovery happened).
        assert!(store.message("m3").is_some());
    }

    #[test]
    fn membership_loss_removes_a_held_row() {
        let (mut store, view) = inbox_view();
        // m2 leaves the inbox mailbox → removed from the view; coverage intact.
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m2".into(),
            projection: summary("m2", "2026-04-28T12:00:00Z", &["archive"]),
            deleted: false,
            count_deltas: vec![],
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
            count_deltas: vec![],
        }]);
        assert!(store.view_rows(view).unwrap().is_empty());
        assert!(store.message("m2").is_none());
    }

    #[test]
    fn count_delta_updates_mailbox_and_is_dirty() {
        let (mut store, _view) = inbox_view();
        store.ingest_batch(vec![StoreUpdate::Message {
            message_id: "m1".into(),
            projection: summary("m1", "2026-04-29T10:00:00Z", &["inbox"]),
            deleted: false,
            count_deltas: vec![CountDelta {
                mailbox_id: "inbox".into(),
                unread_count: 3,
                total_count: 10,
            }],
        }]);
        assert_eq!(store.mailbox("inbox").unwrap().unread_count, 3);
        let dirty = store.drain_dirty();
        assert!(dirty.contains(&DirtyKey::Mailbox("inbox".into())));
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
                count_deltas: vec![CountDelta {
                    mailbox_id: "inbox".into(),
                    unread_count: 2,
                    total_count: 5,
                }],
            },
            StoreUpdate::Message {
                message_id: "m0".into(),
                projection: summary("m0", "2026-04-30T10:00:00Z", &["inbox"]),
                deleted: false,
                count_deltas: vec![],
            },
        ]);
        let dirty = store.drain_dirty();
        let view_dirty_count =
            dirty.iter().filter(|k| matches!(k, DirtyKey::View(v) if v == view)).count();
        assert_eq!(view_dirty_count, 1, "a batch notifies a view once");
        let ids: Vec<&str> =
            store.view_rows(view).unwrap().iter().map(|r| r.message_id.as_str()).collect();
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
            count_deltas: vec![],
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
            count_deltas: vec![],
        }]);
        // The host drives a deferred view via set_view_rows; a raw mutation
        // does not place a row.
        assert!(store.view_rows("search").unwrap().is_empty());
    }
}
