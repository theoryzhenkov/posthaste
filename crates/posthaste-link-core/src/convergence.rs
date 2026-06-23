use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::message::{replay_message, MessageAssertion, MessageFoldState, MessageOutcome};

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

/// One accepted-but-unconfirmed message mutation in the outbox: the near node's
/// optimistic intent, a desired-state assertion over a single message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingMessageMutation {
    pub id: MutationId,
    pub message_id: String,
    pub assertion: MessageAssertion,
}

/// An authoritative update to one message's confirmed base: a new asserted state
/// or a removal ([replication L1 §5.1](../replication/L1.md)).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageBaseUpdate {
    Present(MessageFoldState),
    Removed,
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

/// The message-scope convergence engine of a client-layer link node: a confirmed
/// base of canonical message states plus an ordered outbox of pending mutations.
/// Its visible state is `replay(base, pending)` per message
/// ([replication L1 §5.3](../replication/L1.md)) — optimism is always a pure fold
/// over the confirmed base, never stored as truth.
///
/// This is the pure heart of the replica node (W1). It owns no I/O: persistence,
/// transport, and view recomputation are the node's responsibility; this type
/// only holds the base + outbox and runs the rebase loop.
///
/// @spec docs/replication/L2#5-convergence-in-the-replica
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MessageReplica {
    base: BTreeMap<String, MessageFoldState>,
    pending: Vec<PendingMessageMutation>,
}

impl MessageReplica {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the confirmed base for one message (an applied authoritative
    /// assertion).
    pub fn set_base(&mut self, message_id: impl Into<String>, state: MessageFoldState) {
        self.base.insert(message_id.into(), state);
    }

    /// Swap the **entire** confirmed base for a freshly served one, keeping the
    /// pending outbox intact. Use this when adopting a served view snapshot:
    /// messages absent from the new base leave the base (the served base is
    /// authoritative for the working set), but unconfirmed optimistic mutations
    /// must survive to re-fold over it (`view-is-pure-fold`; they retire only on
    /// settlement, §5.5). Replacing the base without clearing pending is the
    /// base-replace half of the rebase loop ([replication L1 §5.3](../replication/L1.md)).
    pub fn replace_base<I>(&mut self, base: I)
    where
        I: IntoIterator<Item = (String, MessageFoldState)>,
    {
        self.base = base.into_iter().collect();
    }

    /// Drop a message from the confirmed base (authoritative removal).
    pub fn remove_base(&mut self, message_id: &str) {
        self.base.remove(message_id);
    }

    /// Accept an optimistic mutation: append it to the outbox. Idempotent on
    /// mutation id — re-accepting an already-held id is a no-op
    /// ([replication L1 §4.2](../replication/L1.md)).
    pub fn accept(&mut self, mutation: PendingMessageMutation) {
        if self.pending.iter().any(|held| held.id == mutation.id) {
            return;
        }
        self.pending.push(mutation);
    }

    /// Apply an authoritative base update: **replace** the asserted confirmed
    /// states (or remove them). This does not retire pending mutations on its
    /// own — retirement is driven by per-mutation [`settle`](Self::settle), the
    /// shape the contract serves (`RuntimeFrame::MutationSettlement`). Recompute
    /// is `project`, called by the caller for the views it serves.
    ///
    /// Because the local effect is idempotent, the interval where the base
    /// already reflects a still-pending mutation (base updated, settlement not
    /// yet seen) is a visual no-op, so the retire instant cannot flicker
    /// ([replication L1 §5.3](../replication/L1.md)).
    pub fn apply_base_update<I>(&mut self, updates: I)
    where
        I: IntoIterator<Item = (String, MessageBaseUpdate)>,
    {
        for (message_id, update) in updates {
            match update {
                MessageBaseUpdate::Present(state) => {
                    self.base.insert(message_id, state);
                }
                MessageBaseUpdate::Removed => {
                    self.base.remove(&message_id);
                }
            }
        }
    }

    /// Settle a pending mutation by its terminal outcome — the
    /// `retire-on-confirmation` rule ([replication L1 §5.3, §5.5](../replication/L1.md)),
    /// realized per mutation as the contract serves it rather than as a scalar
    /// high-water mark. `Confirmed` drops the pending op (the served base already
    /// reflects it, so this is a no-op visually); `Failed` drops it and reports
    /// `reverted` so the caller surfaces the failure as the view recomputes back
    /// to authoritative state. Out-of-order safe: only the named mutation is
    /// touched. Idempotent: settling an unknown/already-retired id is a no-op.
    pub fn settle(&mut self, id: &MutationId, outcome: SettlementOutcome) -> SettlementResult {
        let before = self.pending.len();
        self.pending.retain(|held| &held.id != id);
        let retired = self.pending.len() != before;
        SettlementResult {
            retired,
            reverted: retired && matches!(outcome, SettlementOutcome::Failed),
        }
    }

    /// The optimistic state of one message: `replay(base, its pending)`. `None`
    /// when the message is not in the confirmed base (not held / not covered).
    pub fn project(&self, message_id: &str) -> Option<MessageOutcome> {
        let base = self.base.get(message_id)?.clone();
        let assertions: Vec<MessageAssertion> = self
            .pending
            .iter()
            .filter(|held| held.message_id == message_id)
            .map(|held| held.assertion.clone())
            .collect();
        Some(replay_message(base, &assertions))
    }

    /// The optimistic state of every held message, with removed messages elided
    /// — the projected working set.
    pub fn project_all(&self) -> BTreeMap<String, MessageFoldState> {
        let mut projected = BTreeMap::new();
        for message_id in self.base.keys() {
            if let Some(MessageOutcome::Present(state)) = self.project(message_id) {
                projected.insert(message_id.clone(), state);
            }
        }
        projected
    }

    pub fn pending(&self) -> &[PendingMessageMutation] {
        &self.pending
    }

    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(keywords: &[&str], mailboxes: &[&str]) -> MessageFoldState {
        MessageFoldState {
            keywords: keywords.iter().map(|k| k.to_string()).collect(),
            mailbox_ids: mailboxes.iter().map(|m| m.to_string()).collect(),
        }
    }

    fn flag(id: &str, message_id: &str) -> PendingMessageMutation {
        PendingMessageMutation {
            id: MutationId(id.into()),
            message_id: message_id.into(),
            assertion: MessageAssertion::SetKeywords {
                add: vec!["$flagged".into()],
                remove: vec![],
            },
        }
    }

    fn present(replica: &MessageReplica, message_id: &str) -> MessageFoldState {
        match replica.project(message_id) {
            Some(MessageOutcome::Present(state)) => state,
            other => panic!("expected present, got {other:?}"),
        }
    }

    #[test]
    fn optimism_shows_before_confirmation() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1", state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        assert_eq!(present(&replica, "m1").keywords, vec!["$flagged".to_string()]);
        assert!(replica.has_pending());
    }

    #[test]
    fn confirmation_retires_pending_and_base_carries_the_effect() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1", state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        // Authority applies op1 and serves the post-state as the new base, then
        // settles op1 confirmed (state-before-event: the base already reflects it).
        replica.apply_base_update([(
            "m1".to_string(),
            MessageBaseUpdate::Present(state(&["$flagged"], &["inbox"])),
        )]);
        let result = replica.settle(&MutationId("op1".into()), SettlementOutcome::Confirmed);
        assert!(result.retired && !result.reverted);
        assert!(!replica.has_pending());
        assert_eq!(present(&replica, "m1").keywords, vec!["$flagged".to_string()]);
    }

    #[test]
    fn base_update_and_still_pending_overlap_is_a_no_op() {
        // The base already reflects op1 but its settlement has not arrived yet:
        // the projection is unchanged (idempotent fold), so no flicker.
        let mut replica = MessageReplica::new();
        replica.set_base("m1", state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        replica.apply_base_update([(
            "m1".to_string(),
            MessageBaseUpdate::Present(state(&["$flagged"], &["inbox"])),
        )]);
        assert!(replica.has_pending());
        assert_eq!(present(&replica, "m1").keywords, vec!["$flagged".to_string()]);
    }

    #[test]
    fn unconfirmed_pending_survives_an_unrelated_base_update() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1", state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        replica.apply_base_update([(
            "m2".to_string(),
            MessageBaseUpdate::Present(state(&[], &["inbox"])),
        )]);
        assert!(replica.has_pending());
        assert_eq!(present(&replica, "m1").keywords, vec!["$flagged".to_string()]);
    }

    #[test]
    fn settle_retires_only_the_named_mutation_out_of_order() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1", state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        replica.accept(flag("op2", "m1"));
        replica.accept(flag("op3", "m1"));
        // Confirm the middle mutation; the others stay pending (out-of-order safe).
        replica.settle(&MutationId("op2".into()), SettlementOutcome::Confirmed);
        let ids: Vec<&str> = replica.pending().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["op1", "op3"]);
    }

    #[test]
    fn failed_settlement_reverts_to_authoritative_state() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1", state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        assert_eq!(present(&replica, "m1").keywords, vec!["$flagged".to_string()]);
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
        replica.set_base("m1", state(&[], &["inbox"]));
        replica.accept(PendingMessageMutation {
            id: MutationId("op1".into()),
            message_id: "m1".into(),
            assertion: MessageAssertion::Destroy,
        });
        assert_eq!(replica.project("m1"), Some(MessageOutcome::Removed));
        assert!(replica.project_all().is_empty());
        // Authority applies the destroy (removes the row) and settles it.
        replica.apply_base_update([("m1".to_string(), MessageBaseUpdate::Removed)]);
        replica.settle(&MutationId("op1".into()), SettlementOutcome::Confirmed);
        assert_eq!(replica.project("m1"), None);
        assert!(!replica.has_pending());
    }

    #[test]
    fn replace_base_swaps_states_but_keeps_pending() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1", state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        // A fresh served base that does not reflect op1, and adds m2.
        replica.replace_base([
            ("m1".to_string(), state(&[], &["inbox"])),
            ("m2".to_string(), state(&[], &["inbox"])),
        ]);
        // Pending survived and re-folds over the new base.
        assert!(replica.has_pending());
        assert_eq!(present(&replica, "m1").keywords, vec!["$flagged".to_string()]);
        assert_eq!(present(&replica, "m2").keywords, Vec::<String>::new());
    }

    #[test]
    fn accept_is_idempotent_on_mutation_id() {
        let mut replica = MessageReplica::new();
        replica.set_base("m1", state(&[], &["inbox"]));
        replica.accept(flag("op1", "m1"));
        replica.accept(flag("op1", "m1"));
        assert_eq!(replica.pending().len(), 1);
    }
}
