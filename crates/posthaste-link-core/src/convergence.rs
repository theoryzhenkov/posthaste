use std::collections::{BTreeMap, HashSet};
use std::fmt::Debug;
use std::marker::PhantomData;

use serde::{Deserialize, Serialize};

use crate::message::{replay_message, MessageAssertion, MessageFoldState};

/// A near-node-minted stable identifier for a mutation
/// ([replication L1 §4.1](../replication/L1.md)). It is preserved as the mutation
/// is forwarded along the chain so the far node's confirmation matches back.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MutationId(pub String);

impl MutationId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Result of folding effects over an entity: its new state, or removed. Removal
/// is terminal — later effects over a removed entity are no-ops. Generic over the
/// state so one convergence engine serves every foldable entity kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome<S> {
    Present(S),
    Removed,
}

/// The fold a foldable entity kind provides: how an ordered list of effects
/// transforms a confirmed base state. One implementation per entity kind
/// (message today; a future mailbox fold reuses the same engine), so optimism is
/// a general property of [`Replica`], not message-specific.
pub trait Convergence {
    type Key: Ord + Clone + Debug;
    type State: Clone + Debug;
    type Effect: Clone + Debug;

    /// Fold an ordered list of effects over a confirmed base state — the
    /// predictor's `replay(base, pending)` ([replication L1 §5.3](../replication/L1.md)).
    fn fold(base: Self::State, effects: &[Self::Effect]) -> Outcome<Self::State>;
}

/// One accepted-but-unconfirmed mutation in the outbox: the near node's optimistic
/// intent, a desired-state effect over one entity (keyed by `key`). Generic field
/// names (`key`/`effect`) so one shape serves every foldable kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingMutation<C: Convergence> {
    pub id: MutationId,
    pub key: C::Key,
    pub effect: C::Effect,
}

/// A pending mutation's terminal outcome, as the far node reports it
/// ([replication L1 §5.5](../replication/L1.md)). The wire form is the contract's
/// `RuntimeFrame::MutationSettlement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettlementOutcome {
    /// The far node applied the mutation; by the state-before-event rule the
    /// served base already reflects it, so retiring the now-redundant pending op
    /// is a visual no-op.
    Confirmed,
    /// The far node rejected the mutation; retiring it lets the recompute revert
    /// the view to authoritative state.
    Failed,
}

/// The effect of settling a pending mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SettlementResult {
    /// Whether a pending mutation with that id was found and removed.
    pub retired: bool,
    /// Whether the settlement was a failure the caller should surface (the view
    /// reverts to authoritative state on recompute).
    pub reverted: bool,
}

/// The convergence engine of a link node: a confirmed base of canonical states
/// plus an ordered outbox of pending mutations, over one foldable entity kind.
/// Its visible state is `replay(base, pending)` per entity
/// ([replication L1 §5.3](../replication/L1.md)) — optimism is always a pure fold
/// over the confirmed base, never stored as truth.
///
/// Generic over a [`Convergence`] so message and any future mailbox fold share
/// one engine across both seams (`predictor-single-crate`). This type owns no
/// I/O: persistence, transport, and view recomputation are the node's job; it
/// only holds the base + outbox and runs the rebase loop.
///
/// @spec docs/replication/client-link/L2#1-the-shared-predictor-crate-posthaste-link-core
#[derive(Clone, Debug)]
pub struct Replica<C: Convergence> {
    base: BTreeMap<C::Key, C::State>,
    pending: Vec<PendingMutation<C>>,
    /// Ids the far node has **confirmed**. An op retires only once it is both
    /// confirmed AND absorbed by the base — so it survives its own local echo
    /// and a concurrent stale re-serve, retiring on the keyed confirmation
    /// rather than on any base update that happens to carry its effect.
    confirmed: HashSet<MutationId>,
    _convergence: PhantomData<C>,
}

impl<C: Convergence> Default for Replica<C> {
    fn default() -> Self {
        Self {
            base: BTreeMap::new(),
            pending: Vec::new(),
            confirmed: HashSet::new(),
            _convergence: PhantomData,
        }
    }
}

impl<C: Convergence> Replica<C> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the confirmed base for one entity (an applied authoritative
    /// assertion).
    pub fn set_base(&mut self, key: C::Key, state: C::State) {
        self.base.insert(key, state);
    }

    /// Drop an entity from the confirmed base (authoritative removal).
    pub fn remove_base(&mut self, key: &C::Key) {
        self.base.remove(key);
    }

    /// Drop every pending op on `key` (authoritative removal of the entity).
    ///
    /// An authoritative delete (expunge, a rule, another client) makes any
    /// pending optimism on that entity moot — the entity is gone, so the op can
    /// neither fold into a base nor revert to one. Without this the op sticks
    /// pending forever (`has_pending` stuck true; the durable outbox grows
    /// unbounded on a delete-heavy workload): the version-gated retire on
    /// `settle(Confirmed)` can't reach a deleted entity (it carries no version
    /// for the gate), and unconfirmed ops are never retired there anyway. Both
    /// confirmed and unconfirmed ops on `key` are dropped, and their confirmed
    /// markers cleared. Idempotent. The caller scopes this to
    /// `apply_message(deleted=true)` — a *never-ingested* entity is not an
    /// authoritative removal, so its deferred pending ops must survive to fold
    /// on a later ingest.
    pub fn remove_pending(&mut self, key: &C::Key) -> bool {
        let remove: Vec<MutationId> = self
            .pending
            .iter()
            .filter(|held| &held.key == key)
            .map(|held| held.id.clone())
            .collect();
        if remove.is_empty() {
            return false;
        }
        self.pending.retain(|held| !remove.contains(&held.id));
        for id in &remove {
            self.confirmed.remove(id);
        }
        true
    }

    /// Remove one pending op by its mutation id (and clear its confirmed
    /// marker), unconditionally — a plain drop with no base or absorption gate.
    /// The co-located near node uses this to retire a forwarded op the instant
    /// its synchronous receipt returns: the far node applied the effect before
    /// the receipt, so the next recompute's base already carries it and there is
    /// no propagation window to gate on. The absorption-gated retire
    /// ([`retire_absorbed`](Self::retire_absorbed)) is for the remote seam, where
    /// the receipt can outrun the base update. Returns whether an op was removed.
    pub fn drop_pending(&mut self, id: &MutationId) -> bool {
        let before = self.pending.len();
        self.pending.retain(|held| &held.id != id);
        self.confirmed.remove(id);
        self.pending.len() != before
    }

    /// Accept an optimistic mutation: append it to the outbox. Idempotent on
    /// mutation id — re-accepting an already-held id is a no-op
    /// ([replication L1 §4.2](../replication/L1.md)).
    pub fn accept(&mut self, mutation: PendingMutation<C>) {
        if self.pending.iter().any(|held| held.id == mutation.id) {
            return;
        }
        self.pending.push(mutation);
    }

    /// Settle a pending mutation by its terminal outcome
    /// ([mutation.notification design](../eph/DESIGN-L2-mutation-notification.md)).
    ///
    /// `Confirmed` does **not** unconditionally drop the op: it marks the op
    /// authority-confirmed, then retires it only if the base already absorbs its
    /// effect (else it stays folded, to retire on a later base update that
    /// carries it). This is the confirmed-gating that keeps an op folded through
    /// its own local echo and a concurrent stale re-serve — retiring on the
    /// keyed confirmation, not on any absorbing ingest. `Failed` drops the op and
    /// reports `reverted` so the view recomputes back to authoritative state.
    /// Out-of-order safe; idempotent on an unknown id.
    pub fn settle(&mut self, id: &MutationId, outcome: SettlementOutcome) -> SettlementResult
    where
        C::State: PartialEq,
    {
        match outcome {
            SettlementOutcome::Confirmed => {
                let Some(key) = self
                    .pending
                    .iter()
                    .find(|held| &held.id == id)
                    .map(|held| held.key.clone())
                else {
                    return SettlementResult::default();
                };
                self.confirmed.insert(id.clone());
                let retired_ids = self.retire_absorbed(&key);
                let retired = !retired_ids.is_empty();
                SettlementResult {
                    retired,
                    reverted: false,
                }
            }
            SettlementOutcome::Failed => {
                let before = self.pending.len();
                self.pending.retain(|held| &held.id != id);
                self.confirmed.remove(id);
                let retired = self.pending.len() != before;
                SettlementResult {
                    retired,
                    reverted: retired,
                }
            }
        }
    }

    /// Mark a mutation authority-confirmed **without** retiring it. The caller
    /// then gates retirement itself via [`retire_absorbed_if`](Self::retire_absorbed_if)
    /// — the message layer does so on the per-message authority version, so an
    /// op accepted at version `v` retires only once a base at a STRICTLY HIGHER
    /// version absorbs it (the equal-version hold that survives the local-echo
    /// + stale re-serve window). Use [`settle`](Self::settle) for the ungated path (it calls this then retires unconditionally).
    pub fn mark_confirmed(&mut self, id: &MutationId) {
        self.confirmed.insert(id.clone());
    }

    /// Retire pending mutations on `key` that are **both authority-confirmed and
    /// absorbed** by the confirmed base (folding the op over the running base
    /// produces no change at its position). Ops are checked in order; a
    /// still-effective op is kept and advances the running state, so a later op
    /// is judged against the state its predecessors produce.
    ///
    /// The confirmed gate is the fix for the local-echo / stale-re-serve flicker
    /// ([mutation.notification design](../eph/DESIGN-L2-mutation-notification.md)):
    /// an **un**confirmed op is never retired here, so it stays folded
    /// (idempotent — invisible) through its own optimistic message.updated echo
    /// and through a concurrent stale provider re-serve; it retires only once the
    /// far node has confirmed it (via [`settle`](Self::settle)) *and* the base
    /// carries the effect. A confirmation that outruns the base update marks the
    /// op confirmed but does not revert it — it retires on the next base update
    /// that absorbs it. Returns whether any op was retired.
    pub fn retire_absorbed(&mut self, key: &C::Key) -> Vec<MutationId>
    where
        C::State: PartialEq,
    {
        self.retire_absorbed_if(key, |_| true)
    }

    /// [`retire_absorbed`](Self::retire_absorbed) with a per-op gate: an op is
    /// retired only if it is confirmed, absorbed, AND `can_retire(&op.id)`.
    /// The message layer passes the version gate here — an op accepted at base
    /// version `v` is `can_retire` only once the current base version is
    /// strictly greater than `v`, so it holds through the equal-version
    /// unconfirmed window (a local move does not bump the provider modseq, so
    /// the moved base and a stale re-serve share the op's accepted-at version;
    /// retiring there would let the stale re-serve clobber membership).
    pub fn retire_absorbed_if<F>(&mut self, key: &C::Key, can_retire: F) -> Vec<MutationId>
    where
        C::State: PartialEq,
        F: Fn(&MutationId) -> bool,
    {
        let Some(base) = self.base.get(key).cloned() else {
            // No base: the entity left the working set (authoritative removal).
            // A confirmed op on it is moot (there is nothing to absorb against,
            // and the authority has both confirmed the op and removed the
            // entity) — retire it so it does not leak. Unconfirmed ops stay.
            let retire: Vec<MutationId> = self
                .pending
                .iter()
                .filter(|held| {
                    &held.key == key && self.confirmed.contains(&held.id) && can_retire(&held.id)
                })
                .map(|held| held.id.clone())
                .collect();
            if retire.is_empty() {
                return Vec::new();
            }
            self.pending.retain(|held| !retire.contains(&held.id));
            for id in &retire {
                self.confirmed.remove(id);
            }
            return retire;
        };
        // First pass (immutable): walk the key's ops in order, collecting those
        // that are confirmed AND absorbed at their position.
        let mut running = base;
        let mut retire: Vec<MutationId> = Vec::new();
        for held in self.pending.iter().filter(|held| &held.key == key) {
            match C::fold(running.clone(), std::slice::from_ref(&held.effect)) {
                Outcome::Present(next) if next == running => {
                    if self.confirmed.contains(&held.id) && can_retire(&held.id) {
                        retire.push(held.id.clone());
                    }
                }
                Outcome::Present(next) => running = next,
                Outcome::Removed => {}
            }
        }
        if retire.is_empty() {
            return Vec::new();
        }
        self.pending.retain(|held| !retire.contains(&held.id));
        for id in &retire {
            self.confirmed.remove(id);
        }
        retire
    }

    /// The optimistic state of one entity: `replay(base, its pending)`. `None`
    /// when the entity is not in the confirmed base (not held / not covered).
    pub fn project(&self, key: &C::Key) -> Option<Outcome<C::State>> {
        let base = self.base.get(key)?.clone();
        let effects: Vec<C::Effect> = self
            .pending
            .iter()
            .filter(|held| &held.key == key)
            .map(|held| held.effect.clone())
            .collect();
        Some(C::fold(base, &effects))
    }

    pub fn pending(&self) -> &[PendingMutation<C>] {
        &self.pending
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

// --- The message fold: the one foldable entity kind today -------------------

/// `Convergence` for message state: the fold is `replay_message` over
/// `MessageFoldState` and `MessageAssertion`. A future mailbox fold adds another
/// impl and reuses [`Replica`] unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageConvergence;

impl Convergence for MessageConvergence {
    type Key = String;
    type State = MessageFoldState;
    type Effect = MessageAssertion;

    fn fold(base: MessageFoldState, effects: &[MessageAssertion]) -> Outcome<MessageFoldState> {
        replay_message(base, effects)
    }
}

/// The message convergence engine — [`Replica<MessageConvergence>`].
pub type MessageReplica = Replica<MessageConvergence>;
/// A pending message mutation — [`PendingMutation<MessageConvergence>`]. Fields
/// are the generic `key` (message id) / `effect` (assertion).
pub type PendingMessageMutation = PendingMutation<MessageConvergence>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::MessageOutcome;

    fn state(keywords: &[&str], mailboxes: &[&str]) -> MessageFoldState {
        MessageFoldState {
            keywords: keywords.iter().map(|k| k.to_string()).collect(),
            mailbox_ids: mailboxes.iter().map(|m| m.to_string()).collect(),
        }
    }

    fn flag(id: &str, message_id: &str) -> PendingMessageMutation {
        PendingMessageMutation {
            id: MutationId(id.into()),
            key: message_id.into(),
            effect: MessageAssertion::SetKeywords {
                add: vec!["$flagged".into()],
                remove: vec![],
            },
        }
    }

    fn present(replica: &MessageReplica, message_id: &str) -> MessageFoldState {
        match replica.project(&message_id.to_string()) {
            Some(MessageOutcome::Present(state)) => state,
            other => panic!("expected present, got {other:?}"),
        }
    }

    #[test]
    fn optimism_shows_before_confirmation() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1".to_string(), state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        assert_eq!(
            present(&replica, "m1").keywords,
            vec!["$flagged".to_string()]
        );
        assert!(replica.has_pending());
    }

    #[test]
    fn confirmation_retires_pending_and_base_carries_the_effect() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1".to_string(), state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        replica.set_base("m1".to_string(), state(&["$flagged"], &["inbox"]));
        let result = replica.settle(&MutationId("op1".into()), SettlementOutcome::Confirmed);
        assert!(result.retired && !result.reverted);
        assert!(!replica.has_pending());
        assert_eq!(
            present(&replica, "m1").keywords,
            vec!["$flagged".to_string()]
        );
    }

    #[test]
    fn base_update_and_still_pending_overlap_is_a_no_op() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1".to_string(), state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        replica.set_base("m1".to_string(), state(&["$flagged"], &["inbox"]));
        assert!(replica.has_pending());
        assert_eq!(
            present(&replica, "m1").keywords,
            vec!["$flagged".to_string()]
        );
    }

    #[test]
    fn unconfirmed_pending_survives_an_unrelated_base_update() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1".to_string(), state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        replica.set_base("m2".to_string(), state(&[], &["inbox"]));
        assert!(replica.has_pending());
        assert_eq!(
            present(&replica, "m1").keywords,
            vec!["$flagged".to_string()]
        );
    }

    #[test]
    fn settle_retires_only_the_named_mutation_out_of_order() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1".to_string(), state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        replica.accept(flag("op2", "m1"));
        replica.accept(flag("op3", "m1"));
        replica.settle(&MutationId("op2".into()), SettlementOutcome::Confirmed);
        let ids: Vec<&str> = replica.pending().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["op1", "op3"]);
    }

    #[test]
    fn failed_settlement_reverts_to_authoritative_state() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1".to_string(), state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        assert_eq!(
            present(&replica, "m1").keywords,
            vec!["$flagged".to_string()]
        );
        let result = replica.settle(&MutationId("op1".into()), SettlementOutcome::Failed);
        assert!(result.retired && result.reverted);
        assert!(present(&replica, "m1").keywords.is_empty());
    }

    #[test]
    fn settling_an_unknown_mutation_is_a_no_op() {
        let mut replica = MessageReplica::new();
        let result = replica.settle(&MutationId("ghost".into()), SettlementOutcome::Confirmed);
        assert_eq!(result, SettlementResult::default());
    }

    #[test]
    fn destroy_then_authoritative_removal() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1".to_string(), state(&[], &["inbox"]));
        replica.accept(PendingMessageMutation {
            id: MutationId("op1".into()),
            key: "m1".into(),
            effect: MessageAssertion::Destroy,
        });
        assert_eq!(
            replica.project(&"m1".to_string()),
            Some(MessageOutcome::Removed)
        );
        replica.remove_base(&"m1".to_string());
        replica.settle(&MutationId("op1".into()), SettlementOutcome::Confirmed);
        assert_eq!(replica.project(&"m1".to_string()), None);
        assert!(!replica.has_pending());
    }

    #[test]
    fn accept_is_idempotent_on_mutation_id() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1".to_string(), state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        replica.accept(flag("op1", "m1"));
        assert_eq!(replica.pending().len(), 1);
    }

    // --- retire_absorbed: the race-free retire trigger -----------------------

    #[test]
    fn retire_absorbed_drops_an_op_the_base_now_carries() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1".to_string(), state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        // Base catches up to the flag (the `message.updated` for the mutation).
        replica.set_base("m1".to_string(), state(&["$flagged"], &["inbox"]));
        // ...but the op only retires once the authority confirms it.
        assert!(
            replica
                .settle(&MutationId("op1".into()), SettlementOutcome::Confirmed)
                .retired
        );
        assert!(!replica.has_pending());
        // The projection still shows the flag — retire did not revert.
        assert_eq!(
            present(&replica, "m1").keywords,
            vec!["$flagged".to_string()]
        );
    }

    #[test]
    fn unconfirmed_op_is_not_retired_even_when_the_base_carries_it() {
        // The Bug-1 fix: a base update that carries the effect (a local echo or a
        // stale provider re-serve) must NOT retire an op the authority has not
        // yet confirmed — it stays folded (idempotent, invisible).
        let mut replica = MessageReplica::new();
        replica.set_base("m1".to_string(), state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        replica.set_base("m1".to_string(), state(&["$flagged"], &["inbox"]));
        assert!(replica.retire_absorbed(&"m1".to_string()).is_empty());
        assert!(replica.has_pending());
    }

    #[test]
    fn retire_absorbed_keeps_an_op_the_base_does_not_carry() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1".to_string(), state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        // Base has NOT caught up (settlement outran the base update).
        assert!(replica.retire_absorbed(&"m1".to_string()).is_empty());
        assert!(replica.has_pending());
        // Optimism survives — no revert window.
        assert_eq!(
            present(&replica, "m1").keywords,
            vec!["$flagged".to_string()]
        );
    }

    #[test]
    fn retire_absorbed_drops_a_leading_absorbed_op_but_keeps_a_later_effective_one() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1".to_string(), state(&[], &["inbox"]));
        // op1: flag; op2: mark read.
        replica.accept(flag("op1", "m1"));
        replica.accept(PendingMessageMutation {
            id: MutationId("op2".into()),
            key: "m1".into(),
            effect: MessageAssertion::SetKeywords {
                add: vec!["$seen".into()],
                remove: vec![],
            },
        });
        // Base caught up to the flag only (op1), not the read (op2). Confirm op1.
        replica.set_base("m1".to_string(), state(&["$flagged"], &["inbox"]));
        assert!(
            replica
                .settle(&MutationId("op1".into()), SettlementOutcome::Confirmed)
                .retired
        );
        let ids: Vec<&str> = replica.pending().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["op2"]);
        // The still-pending read folds over the caught-up base.
        let mut keywords = present(&replica, "m1").keywords;
        keywords.sort();
        assert_eq!(keywords, vec!["$flagged".to_string(), "$seen".to_string()]);
    }

    #[test]
    fn retire_absorbed_is_set_insensitive_on_unsorted_keywords() {
        // A realistic projection: base carries $seen (in provider order) and the
        // flag op's effect. The fold canonicalizes via BTreeSet (sorted); the
        // base keeps provider order. Absorption must compare as SETS, else the
        // op never retires for any message carrying a second keyword.
        let mut replica = MessageReplica::new();
        replica.set_base(
            "m1".to_string(),
            // provider order ($seen before $flagged) — NOT sorted
            state(&["$seen", "$flagged"], &["inbox"]),
        );
        replica.accept(PendingMessageMutation {
            id: MutationId("op1".into()),
            key: "m1".into(),
            effect: MessageAssertion::SetKeywords {
                add: vec!["$flagged".into()],
                remove: vec![],
            },
        });
        assert!(
            replica
                .settle(&MutationId("op1".into()), SettlementOutcome::Confirmed)
                .retired
        );
        assert!(!replica.has_pending());
    }

    #[test]
    fn retire_absorbed_drops_a_no_op_optimism_immediately() {
        // A flag on an already-flagged message: absorbed from the start, so a
        // confirmation clears it with no base update ever arriving.
        let mut replica = MessageReplica::new();
        replica.set_base("m1".to_string(), state(&["$flagged"], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        assert!(
            replica
                .settle(&MutationId("op1".into()), SettlementOutcome::Confirmed)
                .retired
        );
        assert!(!replica.has_pending());
    }

    #[test]
    fn retire_absorbed_on_an_absent_base_is_a_no_op() {
        let mut replica = MessageReplica::new();
        replica.accept(flag("op1", "m1"));
        assert!(replica.retire_absorbed(&"m1".to_string()).is_empty());
        assert!(replica.has_pending());
    }

    #[test]
    fn drop_pending_removes_one_op_unconditionally() {
        // The co-located retire: drop a confirmed op outright, regardless of base
        // or absorption (the far node already applied the effect on receipt).
        let mut replica = MessageReplica::new();
        replica.accept(flag("op1", "m1"));
        replica.accept(flag("op2", "m2"));
        replica.mark_confirmed(&MutationId("op1".into()));
        assert!(replica.drop_pending(&MutationId("op1".into())));
        let ids: Vec<&str> = replica.pending().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["op2"]);
        // Idempotent: dropping an unknown id removes nothing.
        assert!(!replica.drop_pending(&MutationId("op1".into())));
    }

    #[test]
    fn retire_absorbed_leaves_other_keys_pending() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1".to_string(), state(&["$flagged"], &["inbox"]));
        replica.set_base("m2".to_string(), state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        replica.accept(flag("op2", "m2"));
        replica.settle(&MutationId("op1".into()), SettlementOutcome::Confirmed);
        let ids: Vec<&str> = replica.pending().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["op2"]);
    }
}
