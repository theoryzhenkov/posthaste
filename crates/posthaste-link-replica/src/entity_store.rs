//! The reactive entity store core (merged slices 2+3, sub-slices 2a + optimism).
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
//! ## Optimism
//!
//! The store holds a [`MessageReplica`] (the shared convergence engine,
//! `posthaste-link-core`'s `Replica<MessageConvergence>`) over message fold
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
//! Counts are **not** derived: a mailbox entity holds server-authoritative count
//! scalars (the store is partial, so a held-window count is not the true total).
//! Optimism for counts is a later concern (mutation-id-end-to-end); today a count
//! delta from the authority is the only path.
//!
//! Coverage is held as the watermark `W` (the sort key of the last held row;
//! `None` = reaches BOTTOM); the full multi-range `CoverageRange` shape is
//! adopted when jump-to-date lands. The typed `SortKey` supports the
//! `[receivedAt, id]` composite (the default and overwhelmingly common sort);
//! views under a different sort are `Deferred`.
//!
//! @spec docs/eph/DESIGN-L2-client-link-reactive-store
//! @spec docs/eph/PLAN-L2-client-link-reactive-store

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use posthaste_link_core::{
    MessageAssertion, MessageFoldState, MessageReplica, MutationId, Outcome,
    PendingMessageMutation, SettlementOutcome, SettlementResult,
};

/// A changed entity key, reported by [`EntityStore::drain_dirty`].
///
/// Serializes externally-tagged + camelCase so the WASM host can parse a drain
/// as a JSON array of `{"message":id}` / `{"mailbox":id}` / `{"view":id}`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortKey {
    pub received_at: String,
    pub message_id: String,
}

/// A view's membership predicate. `InMailbox`/`All` are **evaluable** (the store
/// self-maintains placement); `Deferred` is not (the host drives deltas).
///
/// Serializes externally-tagged + camelCase: `{"inMailbox":id}` / `"all"` /
/// `"deferred"`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// A mailbox entity: server-authoritative count scalars (the store is partial,
/// so counts are never derived from the held message set).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxEntity {
    pub unread_count: i64,
    pub total_count: i64,
}

/// A message entity: the authoritative `MessageSummary` projection (the base the
/// outbox folds over). The *optimistic* projection a renderer reads is computed
/// on [`EntityStore::message`] — never stored. Internal (not exported): the base
/// projection must not leak past the store's `message()` accessor, which returns
/// the folded state — exposing it would open a second, non-optimistic
/// derivation path.
#[derive(Clone, Debug)]
struct MessageEntity {
    projection: Value,
}

/// A count delta shipped with a message event (atomic per batch — `D3`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountDelta {
    pub mailbox_id: String,
    pub unread_count: i64,
    pub total_count: i64,
}

/// One authoritative update in a batch.
///
/// Serializes externally-tagged + camelCase: `{"message":{messageId,
/// projection, deleted, countDeltas}}` / `{"mailboxCount":{mailboxId,...}}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StoreUpdate {
    /// A message mutation carrying the full projection (enough to evaluate
    /// membership, compute the sort key, and render) plus the affected
    /// mailboxes' new counts (`firehose-carries-rows`, `D2`).
    #[serde(rename_all = "camelCase")]
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
///
/// Holds a [`MessageReplica`] so message optimism is a pure fold over the
/// confirmed base (keywords + mailbox membership); views and the message read
/// derive from the projected state. Bases are seeded per-key on ingest; the
/// outbox is never cleared by a base update, so unconfirmed optimism survives a
/// re-served snapshot and retires only on settlement.
#[derive(Default)]
pub struct EntityStore {
    messages: HashMap<String, MessageEntity>,
    mailboxes: HashMap<String, MailboxEntity>,
    views: HashMap<String, ViewEntity>,
    /// The message convergence engine: confirmed fold states + the optimistic
    /// outbox. Keyed by message id (`MessageConvergence::Key = String`).
    engine: MessageReplica,
    /// Per-op the authority base version captured at accept time, so retirement
    /// can be gated on a STRICTLY HIGHER version. A local move does not bump the
    /// provider modseq, so its same-version echo and a stale re-serve share this
    /// version — retiring there would let the stale re-serve clobber membership.
    /// Absent for ops accepted with no version yet (those retire on the old
    /// confirmed+absorbed rule; opt-in for no-version providers).
    accepted_at: HashMap<MutationId, u64>,
    dirty: HashSet<DirtyKey>,
    /// Ids of ops retired since the last [`drain_retired`] (at settle confirm
    /// or base catch-up). The host clears durable-outbox records only for these
    /// — an un-retired op is still pending in-engine and must survive a reload.
    /// (outbox D)
    retired_buffer: Vec<MutationId>,
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
    pub fn set_view_rows(
        &mut self,
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
                        if !self.messages.contains_key(&row.message_id) {
                            return true;
                        }
                        match self.optimistic_projection(&row.message_id) {
                            Some(projection) => {
                                predicate.matches(&projection)
                                    && in_range(&row.sort_key, &watermark)
                            }
                            None => false,
                        }
                    })
                    .collect()
            }
        };
        if let Some(view) = self.views.get_mut(view_id) {
            view.rows = reconciled;
            view.watermark = watermark;
            self.dirty.insert(DirtyKey::View(view_id.to_string()));
        }
        // Re-apply pending optimistic membership over the freshly-served rows, so
        // a re-serve cannot drop an optimistically-placed row (e.g. an optimistic
        // move-in). No-op for the common archive/move/delete case (no pending op).
        let pending: Vec<String> = self
            .engine
            .pending()
            .iter()
            .map(|op| op.key.clone())
            .collect();
        for message_id in pending {
            if self.messages.contains_key(&message_id) {
                self.rederive_message(&message_id);
            }
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
        // Remember the authority version at accept time so retirement can be
        // gated on a strictly-higher version (the equal-version hold that
        // survives the local move's same-modseq echo + a stale re-serve).
        if let Some(version) = self
            .messages
            .get(message_id)
            .and_then(|entity| authority_version(&entity.projection))
        {
            self.accepted_at.insert(mutation_id.clone(), version);
        }
        self.engine.accept(PendingMessageMutation {
            id: mutation_id,
            key: message_id.to_string(),
            effect: assertion,
        });
        if self.messages.contains_key(message_id) {
            self.rederive_message(message_id);
        }
    }

    /// Settle a pending mutation by its terminal outcome.
    ///
    /// `Confirmed` does **not** revert: it retires the op only if the confirmed
    /// base already carries its effect (`retire_absorbed`), otherwise it leaves
    /// the op folded for the authoritative `message.updated` to retire. This is
    /// the race fix ([mutation.notification design](../eph/DESIGN-L2-mutation-notification.md)):
    /// a confirmation that outruns the base update can no longer flip the
    /// projection back to a stale base. `Failed` retires the op unconditionally
    /// and the projection reverts to authoritative state (the rejection path).
    /// Out-of-order safe; idempotent on an unknown id.
    pub fn settle(
        &mut self,
        mutation_id: &MutationId,
        outcome: SettlementOutcome,
    ) -> SettlementResult {
        // Look up the affected message before settling so we can re-fold just it.
        let key = self
            .engine
            .pending()
            .iter()
            .find(|held| &held.id == mutation_id)
            .map(|held| held.key.clone());
        let result = match outcome {
            SettlementOutcome::Confirmed => {
                // Mark confirmed, but retire only ops whose current base version
                // is STRICTLY HIGHER than at accept. A local move does not bump
                // the provider modseq, so its same-version echo would absorb +
                // retire the op prematurely — letting a later equal-version stale
                // re-serve clobber membership. The op stays folded through that
                // window, retiring only on the real modseq bump.
                self.engine.mark_confirmed(mutation_id);
                let mut retired = false;
                if let Some(message_id) = key.as_ref() {
                    let current = self
                        .messages
                        .get(message_id.as_str())
                        .and_then(|e| authority_version(&e.projection));
                    let can_retire = self.retireable_ops(message_id.as_str(), current);
                    let retired_ids = self.engine.retire_absorbed_if(
                        message_id,
                        |id| can_retire.contains(id),
                    );
                    retired = !retired_ids.is_empty();
                    self.retired_buffer.extend(retired_ids);
                    if retired {
                        self.prune_accepted_at(message_id.as_str());
                    }
                }
                SettlementResult { retired, reverted: false }
            }
            SettlementOutcome::Failed => {
                self.accepted_at.remove(mutation_id);
                let result = self.engine.settle(mutation_id, outcome);
                if result.retired {
                    self.retired_buffer.push(mutation_id.clone());
                }
                result
            }
        };
        if let Some(message_id) = key {
            if self.messages.contains_key(&message_id) {
                self.rederive_message(&message_id);
            }
        }
        result
    }

    /// The pending ops on `message_id` that may retire at `current_version`: an
    /// op retires only if it was accepted with no version tracked (opt-in for
    /// no-version providers — the old confirmed+absorbed rule) OR the current
    /// base version is STRICTLY HIGHER than the version captured at accept (a
    /// real provider modseq bump, not the local move's same-modsec echo / a
    /// stale re-serve). This is the equal-version hold.
    fn retireable_ops(
        &self,
        message_id: &str,
        current_version: Option<u64>,
    ) -> HashSet<MutationId> {
        self.engine
            .pending()
            .iter()
            .filter(|held| held.key.as_str() == message_id)
            .filter(|held| match self.accepted_at.get(&held.id) {
                None => true,
                Some(at) => current_version.is_some_and(|cur| cur > *at),
            })
            .map(|held| held.id.clone())
            .collect()
    }

    /// Drop `accepted_at` entries for ops no longer pending on `message_id`
    /// (retired/failed), so the map does not leak across the outbox lifecycle.
    fn prune_accepted_at(&mut self, message_id: &str) {
        let live: HashSet<MutationId> = self
            .engine
            .pending()
            .iter()
            .filter(|held| held.key.as_str() == message_id)
            .map(|held| held.id.clone())
            .collect();
        self.accepted_at.retain(|id, _| live.contains(id));
    }

    /// Whether any optimistic mutation is still pending (drives optimistic-UI
    /// affordances and settle-driven dirty drains).
    pub fn has_pending(&self) -> bool {
        self.engine.has_pending()
    }

    fn apply_message(&mut self, message_id: &str, projection: &Value, deleted: bool) {
        if deleted {
            self.messages.remove(message_id);
            self.engine.remove_base(&message_id.to_string());
            // Authoritative removal: purge any pending optimism on this entity.
            // It is gone — the op can neither fold into a base nor revert to
            // one — so without this it leaks pending forever (has_pending stuck
            // true; the durable outbox grows unbounded on delete-heavy
            // workloads). settle(Confirmed)'s version-gated retire can't reach
            // a deleted entity (no version for the gate), and unconfirmed ops
            // are never retired there anyway. Scoped to deleted=true — a
            // *never-ingested* entity is not an authoritative removal; its
            // deferred pending must survive to fold on a later ingest.
            self.engine.remove_pending(&message_id.to_string());
            self.remove_message_from_views(message_id);
        } else {
            // Staleness guard: reject a base whose authority-state version is
            // STRICTLY OLDER than the held one. A late provider re-serve carrying
            // a snapshot that predates the current state (the post-confirm
            // flicker tail) must not clobber a newer confirmed base. Equal
            // versions are idempotent (accepted); absent versions (no provider
            // version yet) skip the guard, so it is inert until the runtime
            // stamps `version` on the projection.
            if let (Some(incoming), Some(held)) = (
                authority_version(projection),
                self.messages
                    .get(message_id)
                    .and_then(|entity| authority_version(&entity.projection)),
            ) {
                if incoming < held {
                    return;
                }
            }
            self.messages.insert(
                message_id.to_string(),
                MessageEntity {
                    projection: projection.clone(),
                },
            );
            self.engine.set_base(
                message_id.to_string(),
                fold_state_from_projection(projection),
            );
            // A base update retires any pending op the new base now carries
            // (the race-free happy-path retire) — but only at a STRICTLY HIGHER
            // version: an equal-version base (the local move's same-modseq echo,
            // or a stale re-serve) must NOT retire the op, so it stays folded and
            // holds membership through the unconfirmed window.
            let can_retire =
                self.retireable_ops(message_id, authority_version(projection));
            let retired_ids = self
                .engine
                .retire_absorbed_if(&message_id.to_string(), |id| {
                    can_retire.contains(id)
                });
            let retired = !retired_ids.is_empty();
            self.retired_buffer.extend(retired_ids);
            if retired {
                self.prune_accepted_at(message_id);
            }
            self.rederive_message(message_id);
        }
        self.dirty.insert(DirtyKey::Message(message_id.to_string()));
    }

    fn apply_count_delta(&mut self, delta: &CountDelta) {
        let mailbox = self.mailboxes.entry(delta.mailbox_id.clone()).or_default();
        mailbox.unread_count = delta.unread_count;
        mailbox.total_count = delta.total_count;
        self.dirty.insert(DirtyKey::Mailbox(delta.mailbox_id.clone()));
    }

    /// Re-evaluate a held message's placement across every evaluable view from
    /// its projected (folded) state, and mark it dirty. Assumes the message is
    /// held (in `messages`); callers gate on that so an un-ingested message's
    /// authoritative rows are left untouched (its pending folds in on ingest).
    fn rederive_message(&mut self, message_id: &str) {
        let optimistic = self.optimistic_projection(message_id);
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
                    predicate.matches(proj) && in_range(&row.sort_key, &watermark)
                }
                _ => false,
            };
            let changed = match (place, was_present) {
                (true, Some(idx)) => {
                    let row = row_opt.clone().expect("row present when placed");
                    if view.rows[idx] != row {
                        view.rows.remove(idx);
                        insert_sorted(&mut view.rows, row, direction);
                        true
                    } else {
                        false
                    }
                }
                (true, None) => {
                    insert_sorted(
                        &mut view.rows,
                        row_opt.clone().expect("row present when placed"),
                        direction,
                    );
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
        self.dirty.insert(DirtyKey::Message(message_id.to_string()));
    }

    /// Drop a message's row from every evaluable view (authoritative removal).
    fn remove_message_from_views(&mut self, message_id: &str) {
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
    }

    /// The optimistic projection for a message: the authoritative base with the
    /// pending outbox folded over its keywords/mailboxes, or `None` if the
    /// message is not held or has been optimistically destroyed.
    fn optimistic_projection(&self, message_id: &str) -> Option<Value> {
        let base = self.messages.get(message_id)?.projection.clone();
        match self.engine.project(&message_id.to_string())? {
            Outcome::Present(state) => Some(apply_fold_to_projection(base, &state)),
            Outcome::Removed => None,
        }
    }

    /// Read a message's optimistic projection (None if not held or destroyed).
    pub fn message(&self, message_id: &str) -> Option<Value> {
        self.optimistic_projection(message_id)
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

    /// Drain the ids of ops retired since the last drain (at settle confirm or
    /// at base catch-up). The host clears the corresponding durable-outbox
    /// records only for these — an un-retired op is still pending in-engine and
    /// must survive a reload to be replayed. (outbox D)
    pub fn drain_retired(&mut self) -> Vec<MutationId> {
        std::mem::take(&mut self.retired_buffer)
    }
}

/// The per-message authority-state version of a projection, if present — an
/// opaque, provider-causality-ordered counter (IMAP MODSEQ / JMAP object state,
/// stamped by the runtime). Compared opaquely by [`EntityStore::apply_message`]'s
/// staleness guard; `None` (no version yet) disables the guard for that message.
fn authority_version(projection: &Value) -> Option<u64> {
    projection.get("version").and_then(Value::as_u64)
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

/// Read the foldable canonical state (keywords + mailbox ids) out of a row's
/// presentation projection. Absent/!array fields read as empty.
pub fn fold_state_from_projection(projection: &Value) -> MessageFoldState {
    MessageFoldState {
        keywords: string_array(projection.get("keywords")),
        mailbox_ids: string_array(projection.get("mailboxIds")),
    }
}

/// Write the folded canonical state back into a presentation projection,
/// re-deriving the read/flag display flags from the keywords and preserving
/// every other field.
pub fn apply_fold_to_projection(mut projection: Value, state: &MessageFoldState) -> Value {
    if let Value::Object(map) = &mut projection {
        map.insert(
            "keywords".to_string(),
            Value::Array(state.keywords.iter().cloned().map(Value::String).collect()),
        );
        map.insert(
            "mailboxIds".to_string(),
            Value::Array(
                state
                    .mailbox_ids
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
        map.insert(
            "isRead".to_string(),
            Value::Bool(state.keywords.iter().any(|keyword| keyword == "$seen")),
        );
        map.insert(
            "isFlagged".to_string(),
            Value::Bool(state.keywords.iter().any(|keyword| keyword == "$flagged")),
        );
    }
    projection
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_link_core::{MessageAssertion, MutationId, SettlementOutcome};
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
            count_deltas: vec![],
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
            count_deltas: vec![],
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
            count_deltas: vec![],
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
        let ids: Vec<&str> =
            store.view_rows(view).unwrap().iter().map(|r| r.message_id.as_str()).collect();
        assert_eq!(ids, vec!["m2"]);
        assert!(store.has_pending());
        let dirty = store.drain_dirty();
        assert!(dirty.contains(&DirtyKey::Message("m2".into())));
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
            count_deltas: vec![],
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
        assert!(store.engine.pending().is_empty());

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
        assert!(store.has_pending(), "op still holds through the stale re-serve");

        // Provider confirms with modseq+1 ([archive]@v6): strictly higher → retire.
        ingest_m2_mailbox_v(&mut store, &["archive"], 6);
        assert!(!store.has_pending(), "op retires on the real modseq bump");
        assert!(store.view_rows(view).unwrap().is_empty());
        assert_eq!(store.message("m2").unwrap()["mailboxIds"], json!(["archive"]));
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
            count_deltas: vec![],
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
        assert_eq!(store.message("m2").unwrap()["mailboxIds"], json!(["archive"]));
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
            count_deltas: vec![],
        }]);

        // m2's optimism survived the rebase (its pending was not cleared).
        assert_eq!(store.message("m2").unwrap()["isFlagged"], json!(true));
        assert!(store.has_pending());
        let ids: Vec<&str> =
            store.view_rows(view).unwrap().iter().map(|r| r.message_id.as_str()).collect();
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
        assert_eq!(store.view_rows(view).unwrap().len(), 1, "row left untouched");

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
            count_deltas: vec![CountDelta {
                mailbox_id: "inbox".into(),
                unread_count: 3,
                total_count: 10,
            }],
        };
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(
            json["message"]["messageId"], json!("m1"),
            "message update is externally-tagged + camelCase"
        );
        assert_eq!(json["message"]["deleted"], json!(false));
        assert_eq!(
            json["message"]["countDeltas"][0]["mailboxId"],
            json!("inbox")
        );
        // Round-trips back unchanged.
        let back: StoreUpdate = serde_json::from_value(json).unwrap();
        assert_eq!(back, update);
    }

    #[test]
    fn mailbox_count_update_round_trips() {
        let update = StoreUpdate::MailboxCount(CountDelta {
            mailbox_id: "inbox".into(),
            unread_count: 1,
            total_count: 2,
        });
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["mailboxCount"]["unreadCount"], json!(1));
        let back: StoreUpdate = serde_json::from_value(json).unwrap();
        assert_eq!(back, update);
    }

    #[test]
    fn dirty_key_serializes_externally_tagged() {
        // The host drains dirty as a JSON array of these.
        assert_eq!(
            serde_json::to_value(DirtyKey::Message("m1".into())).unwrap(),
            json!({"message": "m1"})
        );
        assert_eq!(
            serde_json::to_value(DirtyKey::Mailbox("inbox".into())).unwrap(),
            json!({"mailbox": "inbox"})
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
            serde_json::to_value(ViewPredicate::InMailbox("inbox".into())).unwrap(),
            json!({"inMailbox": "inbox"})
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
