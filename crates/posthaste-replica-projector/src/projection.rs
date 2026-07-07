//! The view **projection** layer of the entity store (RFC D36 layer 2, the
//! shared projector of D38): keyed view rows, membership predicates, coverage
//! windowing, sort keys, and the dirty-key reactivity bookkeeping.
//!
//! Mailbox COUNTS are not held here (RFC-L2-count-unification): the store is
//! partial, so it can never derive true counts, and the client reads them
//! through react-query invalidation against the runtime's canonical
//! trigger-maintained mailbox rows instead of applying per-event deltas.
//!
//! It reads folded (optimistic) state through the mechanism layer
//! ([`crate::mechanism`]) and never touches the outbox lifecycle itself —
//! accept/settle/retire are the mechanism's; rows and windows are this
//! layer's. Wire-agnostic by requirement (RFC D37): it does not know whether
//! its views render directly or get framed/linked/paginated.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mechanism::ReplicaMechanism;

/// A changed entity key, reported by `EntityStore::drain_dirty`.
///
/// Serializes externally-tagged + camelCase so the WASM host can parse a drain
/// as a JSON array of `{"message":id}` / `{"view":id}`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DirtyKey {
    Message(String),
    View(String),
}

/// The outcome of re-evaluating a message's placement in one view, used to
/// drive dirty-marking + reverse-index maintenance after the row borrow ends.
enum Placement {
    /// The row is in the view after rederive (newly placed, reordered, or a
    /// content-only change): mark the view dirty + record membership.
    Present,
    /// The row was held but no longer matches: mark dirty + drop membership.
    Removed,
    /// Not a member before or after: nothing to do.
    Absent,
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
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortKey {
    pub received_at: String,
    pub message_id: String,
}

/// A view's membership predicate. `InMailboxes`/`All` are **evaluable** (the
/// store self-maintains placement); `Deferred` is not (the host drives deltas).
///
/// `InMailboxes` is set-intersection: a message matches if its `mailboxIds`
/// intersect the predicate's set. A concrete-folder view holds a one-element
/// set; a role smart mailbox (e.g. "All Inboxes") holds the role's mailbox in
/// every account.
///
/// Serializes externally-tagged + camelCase: `{"inMailboxes":[id,..]}` /
/// `"all"` / `"deferred"`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewPredicate {
    InMailboxes(Vec<String>),
    All,
    Deferred,
}

impl ViewPredicate {
    /// Whether a message matches this view's filter. `Deferred` returns `true`
    /// (the store does not decide; the host places via deltas).
    fn matches(&self, projection: &Value) -> bool {
        match self {
            ViewPredicate::InMailboxes(mailbox_ids) => projection
                .get("mailboxIds")
                .and_then(Value::as_array)
                .map(|ids| {
                    ids.iter().any(|id| {
                        id.as_str()
                            .is_some_and(|id| mailbox_ids.iter().any(|m| m == id))
                    })
                })
                .unwrap_or(false),
            ViewPredicate::All => true,
            ViewPredicate::Deferred => true,
        }
    }

    fn is_evaluable(&self) -> bool {
        !matches!(self, ViewPredicate::Deferred)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// The projection half of the entity store: views (ordered rows + coverage),
/// the reverse membership index, and the dirty set.
#[derive(Default)]
pub(crate) struct ViewProjection {
    pub(crate) views: HashMap<String, ViewEntity>,
    /// Reverse index `messageId -> set of viewIds the message currently appears
    /// in`, kept in sync as rows are inserted/removed/rederived. A content-only
    /// `rederive_message` (a flag/read toggle that leaves the sort key — and so
    /// the row tuple — unchanged) marks the OWNING views dirty through this
    /// index, so the drained `View` set is trustworthy and complete: the host
    /// re-projects only the views a change actually touched, never all of them
    /// (`adapter-reproject-all`). Empty sets are pruned so membership is exact.
    pub(crate) message_views: HashMap<String, HashSet<String>>,
    pub(crate) dirty: HashSet<DirtyKey>,
}

impl ViewProjection {
    /// Register a view with its predicate, sort, and initial coverage watermark.
    /// Marks the view dirty so the host re-reads its rows.
    pub(crate) fn register_view(
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

    /// Adopt a served snapshot / page / resync for a view — but **reconcile, not
    /// clobber**. The runtime's full-view re-serve is a *second* membership source
    /// for an evaluable view the store already self-maintains from `message.updated`;
    /// a stale re-serve must not re-add a row the client's (version-guarded) base
    /// says has moved out, nor drop a row the client has optimistically placed. So
    /// for an evaluable predicate: (1) keep only served rows whose current
    /// optimistic projection still matches the predicate + coverage — a row the
    /// folded base has moved out of the view, or destroyed, is dropped; then
    /// (2) re-apply pending optimistic placements over the result. A `Deferred`
    /// view is host-driven, so its served rows are trusted as-is.
    ///
    /// This establishes the invariant: in an evaluable view a row is present iff
    /// its folded base matches the predicate — making the store's self-maintained
    /// membership authoritative, so a stale `viewReplace`/`viewSnapshot` can no
    /// longer clobber it (the move/delete/archive flicker,
    /// [reserve-clobbers-optimism](../../issues/L2-reserve-clobbers-optimism)).
    /// Does not touch the message base or outbox.
    pub(crate) fn set_view_rows(
        &mut self,
        mechanism: &ReplicaMechanism,
        view_id: &str,
        rows: Vec<ViewRow>,
        watermark: Option<SortKey>,
    ) {
        let reconciled = match self.views.get(view_id) {
            None => return,
            Some(view) if !view.predicate.is_evaluable() => rows,
            Some(view) => {
                let predicate = view.predicate.clone();
                rows.into_iter()
                    .filter(|row| {
                        // No held base yet (the served snapshot precedes its
                        // ingest): trust the served row — ingest + rederive will
                        // re-evaluate it. A held base that no longer matches
                        // (moved out) or is destroyed (project → None) is dropped.
                        if !mechanism.is_held(&row.message_id) {
                            return true;
                        }
                        match mechanism.optimistic_projection(&row.message_id) {
                            Some(projection) => {
                                predicate.matches(&projection)
                                    && in_range(&row.sort_key, &watermark, view.sort_direction)
                            }
                            None => false,
                        }
                    })
                    .collect()
            }
        };
        let (old_members, new_members) = match self.views.get_mut(view_id) {
            None => return,
            Some(view) => {
                let old: Vec<String> = view.rows.iter().map(|r| r.message_id.clone()).collect();
                let new: Vec<String> = reconciled.iter().map(|r| r.message_id.clone()).collect();
                view.rows = reconciled;
                view.watermark = watermark;
                self.dirty.insert(DirtyKey::View(view_id.to_string()));
                (old, new)
            }
        };
        // Rebuild this view's slice of the reverse index: drop its old members,
        // then record the served set (rederive below re-derives optimistic
        // placement, adjusting the index as rows move).
        for message_id in &old_members {
            self.index_remove(message_id, view_id);
        }
        for message_id in &new_members {
            self.index_insert(message_id, view_id);
        }
        // Re-apply pending optimistic membership over the freshly-served rows, so
        // a re-serve cannot drop an optimistically-placed row (e.g. an optimistic
        // move-in). No-op for the common archive/move/delete case (no pending op).
        let pending: Vec<String> = mechanism
            .pending()
            .iter()
            .map(|op| op.key.clone())
            .collect();
        for message_id in pending {
            if mechanism.is_held(&message_id) {
                self.rederive_message(mechanism, &message_id);
            }
        }
    }

    pub(crate) fn close_view(&mut self, view_id: &str) {
        if let Some(view) = self.views.remove(view_id) {
            for row in &view.rows {
                let message_id = row.message_id.clone();
                self.index_remove(&message_id, view_id);
            }
        }
    }

    /// Record that `message_id` now appears in `view_id` (reverse index).
    fn index_insert(&mut self, message_id: &str, view_id: &str) {
        self.message_views
            .entry(message_id.to_string())
            .or_default()
            .insert(view_id.to_string());
    }

    /// Drop `message_id`'s membership in `view_id`, pruning an emptied set so the
    /// index never reports a message as appearing in a view it has left.
    fn index_remove(&mut self, message_id: &str, view_id: &str) {
        if let Some(views) = self.message_views.get_mut(message_id) {
            views.remove(view_id);
            if views.is_empty() {
                self.message_views.remove(message_id);
            }
        }
    }

    /// Re-evaluate a held message's placement across every evaluable view from
    /// its projected (folded) state, and mark it dirty. Assumes the message is
    /// held (in the mechanism's bases); callers gate on that so an un-ingested
    /// message's authoritative rows are left untouched (its pending folds in on
    /// ingest).
    pub(crate) fn rederive_message(&mut self, mechanism: &ReplicaMechanism, message_id: &str) {
        let optimistic = mechanism.optimistic_projection(message_id);
        let row_opt: Option<ViewRow> = optimistic.as_ref().map(|proj| ViewRow {
            row_key: row_key_of(proj, message_id),
            message_id: message_id.to_string(),
            sort_key: sort_key_of(proj, message_id),
        });
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
            let place = match (&row_opt, &optimistic) {
                (Some(row), Some(proj)) => {
                    predicate.matches(proj) && in_range(&row.sort_key, &watermark, direction)
                }
                _ => false,
            };
            // `Present` covers both a reorder (tuple changed) and a content-only
            // change (same sort key, new flags/read state): the message remains a
            // member, so the view's projected JSON moved — mark it dirty. This is
            // the fix the host's dirty-View set was missing; the JSON-diff gate
            // stays the safety net for a true no-op rederive.
            let placement = match (place, was_present) {
                (true, Some(idx)) => {
                    let row = row_opt.clone().expect("row present when placed");
                    if view.rows[idx] != row {
                        view.rows.remove(idx);
                        insert_sorted(&mut view.rows, row, direction);
                    }
                    Placement::Present
                }
                (true, None) => {
                    insert_sorted(
                        &mut view.rows,
                        row_opt.clone().expect("row present when placed"),
                        direction,
                    );
                    Placement::Present
                }
                (false, Some(idx)) => {
                    view.rows.remove(idx);
                    Placement::Removed
                }
                (false, None) => Placement::Absent,
            };
            match placement {
                Placement::Present => {
                    self.index_insert(message_id, &view_id);
                    self.dirty.insert(DirtyKey::View(view_id));
                }
                Placement::Removed => {
                    self.index_remove(message_id, &view_id);
                    self.dirty.insert(DirtyKey::View(view_id));
                }
                Placement::Absent => {}
            }
        }
        self.dirty.insert(DirtyKey::Message(message_id.to_string()));
    }

    /// Drop a message's row from every evaluable view (authoritative removal).
    pub(crate) fn remove_message_from_views(&mut self, message_id: &str) {
        let view_ids: Vec<String> = self.views.keys().cloned().collect();
        for view_id in view_ids {
            let view = match self.views.get_mut(&view_id) {
                Some(v) => v,
                None => continue,
            };
            if !view.predicate.is_evaluable() {
                continue;
            }
            if let Some(idx) = view.rows.iter().position(|r| r.message_id == message_id) {
                view.rows.remove(idx);
                self.dirty.insert(DirtyKey::View(view_id));
            }
        }
        // The message is gone from every view; drop its whole reverse-index entry.
        self.message_views.remove(message_id);
    }

    /// Mark a message's key dirty (its projection moved).
    pub(crate) fn mark_message_dirty(&mut self, message_id: &str) {
        self.dirty.insert(DirtyKey::Message(message_id.to_string()));
    }

    /// Read a view's rows.
    pub(crate) fn view_rows(&self, view_id: &str) -> Option<&[ViewRow]> {
        self.views.get(view_id).map(|v| v.rows.as_slice())
    }

    /// Drain the keys changed since the last drain. The host re-reads these.
    pub(crate) fn drain_dirty(&mut self) -> Vec<DirtyKey> {
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
fn in_range(sort_key: &SortKey, watermark: &Option<SortKey>, direction: SortDirection) -> bool {
    match watermark {
        None => true,
        Some(w) => match (direction, sort_key.cmp(w)) {
            (SortDirection::Desc, Ordering::Greater | Ordering::Equal) => true,
            (SortDirection::Desc, Ordering::Less) => false,
            (SortDirection::Asc, Ordering::Less | Ordering::Equal) => true,
            (SortDirection::Asc, Ordering::Greater) => false,
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
