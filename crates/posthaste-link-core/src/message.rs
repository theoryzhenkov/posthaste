use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::convergence::Outcome;

/// The subset of canonical message state a mutation's local effect transforms:
/// keyword set and mailbox membership. Renderer-facing derivations (is_read,
/// is_flagged) are computed by the caller from `keywords`; the predictor only
/// moves keywords and membership so it stays independent of presentation shape.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MessageFoldState {
    pub keywords: Vec<String>,
    pub mailbox_ids: Vec<String>,
}

impl PartialEq for MessageFoldState {
    /// **Set-insensitive** equality: two fold states are equal iff they carry
    /// the same keyword set and the same mailbox set, regardless of order.
    ///
    /// This is load-bearing for `retire_absorbed`'s absorption test
    /// (`fold(base, [op]) == base`). The confirmed base is built from the served
    /// projection in **provider order** (`fold_state_from_projection`), while the
    /// fold canonicalizes via `BTreeSet` (sorted). A derived, order-sensitive
    /// `PartialEq` therefore reports "changed" for any ≥-2-keyword message whose
    /// provider order isn't already sorted (e.g. anything carrying `$seen`), so
    /// the op never absorbs and never retires — the intermittent
    /// stuck-optimism bug. Comparing as sets is also the correct semantics: a
    /// fold state *is* a keyword set + a mailbox set.
    fn eq(&self, other: &Self) -> bool {
        as_set(&self.keywords) == as_set(&other.keywords)
            && as_set(&self.mailbox_ids) == as_set(&other.mailbox_ids)
    }
}

impl Eq for MessageFoldState {}

fn as_set(items: &[String]) -> BTreeSet<&str> {
    items.iter().map(String::as_str).collect()
}

/// A symmetric add/remove delta over one facet of a message's mutable state.
/// `inverse` swaps added↔removed. Reused for keywords and mailbox membership.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeywordDelta {
    #[serde(default)]
    pub added: Vec<String>,
    #[serde(default)]
    pub removed: Vec<String>,
}

impl KeywordDelta {
    /// Swap added↔removed — the inverse applies the opposite transition.
    pub fn inverse(&self) -> Self {
        Self {
            added: self.removed.clone(),
            removed: self.added.clone(),
        }
    }

    /// Whether the delta carries no change. An empty diff's inverse is itself a
    /// no-op, so a non-invertible (no-op) mutation records nothing.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

/// An invertible change-diff over a message's mutable state: keywords + mailbox
/// membership, each an add/remove pair. `inverse` swaps both; applying
/// `inverse(diff)` over `curr` reconstructs `prev`, so the wire carries
/// `curr + diff` rather than `curr + prev`. This is the unit the runtime records
/// per reversible mutation and broadcasts for undo/redo — undo applies
/// `inverse(diff)`, redo applies `diff` — so undo/redo become ordinary optimistic
/// mutations through the existing outbox + replay guard (no command-based stack,
/// no opaque `mutation.undo`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageChangeDiff {
    #[serde(default)]
    pub keywords: KeywordDelta,
    #[serde(default)]
    pub mailboxes: KeywordDelta,
}

impl MessageChangeDiff {
    /// Swap added↔removed for both facets — the diff that reverses this one.
    pub fn inverse(&self) -> Self {
        Self {
            keywords: self.keywords.inverse(),
            mailboxes: self.mailboxes.inverse(),
        }
    }

    /// The uniform delta between two fold states: `added = curr \ prev`,
    /// `removed = prev \ curr`, per facet. Stable sorted order (BTreeSet) so a
    /// diff is order-independent and compares equal across reorderings. The
    /// runtime captures a mutation's diff from a before/after read, so this is
    /// correct for every message mutation — including role moves, which need no
    /// near-node role→mailbox resolution (the after-state already reflects it).
    pub fn from_before_after(prev: &MessageFoldState, curr: &MessageFoldState) -> Self {
        Self {
            keywords: delta(&prev.keywords, &curr.keywords),
            mailboxes: delta(&prev.mailbox_ids, &curr.mailbox_ids),
        }
    }

    /// Whether the diff carries no change for either facet.
    pub fn is_empty(&self) -> bool {
        self.keywords.is_empty() && self.mailboxes.is_empty()
    }
}

/// The add/remove delta between two sets: `added = curr \ prev`,
/// `removed = prev \ curr`, stable-sorted.
fn delta(prev: &[String], curr: &[String]) -> KeywordDelta {
    let prev_set: BTreeSet<&String> = prev.iter().collect();
    let curr_set: BTreeSet<&String> = curr.iter().collect();
    KeywordDelta {
        added: curr_set
            .difference(&prev_set)
            .map(|item| (*item).clone())
            .collect(),
        removed: prev_set
            .difference(&curr_set)
            .map(|item| (*item).clone())
            .collect(),
    }
}

/// A named mutation's local effect on one message. Keyword changes are an
/// add/remove pair (idempotent: adding a present keyword or removing an absent
/// one is a no-op); mailbox membership is a full desired-state replace; destroy
/// removes the message; `ApplyDiff` applies an invertible add/remove diff to
/// both facets at once — the undo/redo vehicle (undo applies `inverse(diff)`,
/// redo applies `diff`). These mirror the outbox operation kinds the runtime
/// enqueues.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MessageAssertion {
    SetKeywords {
        #[serde(default)]
        add: Vec<String>,
        #[serde(default)]
        remove: Vec<String>,
    },
    ReplaceMailboxes {
        mailbox_ids: Vec<String>,
    },
    Destroy,
    /// Apply an invertible diff: add/remove keywords and add/remove mailboxes as
    /// deltas (set semantics, idempotent).
    ApplyDiff {
        diff: MessageChangeDiff,
    },
}

/// Result of folding assertions over a message: its new state, or removed
/// (destroyed). Removal is terminal — later assertions over a removed message
/// are no-ops. An alias for the generic [`Outcome<MessageFoldState>`] so the
/// message fold shares the convergence engine's result type.
pub type MessageOutcome = Outcome<MessageFoldState>;

/// Apply one assertion's local effect to a message state.
///
/// Keyword folding uses set semantics with a stable sorted order, so folding the
/// same assertion twice is a no-op (idempotent), which is what makes the
/// base-updated-and-still-pending interval invisible during convergence
/// ([replication L1 §4.4, §5.3](../replication/L1.md)).
pub fn apply_message_assertion(
    mut state: MessageFoldState,
    assertion: &MessageAssertion,
) -> MessageOutcome {
    match assertion {
        MessageAssertion::SetKeywords { add, remove } => {
            let mut keywords: BTreeSet<String> = state.keywords.into_iter().collect();
            for keyword in add {
                keywords.insert(keyword.clone());
            }
            for keyword in remove {
                keywords.remove(keyword);
            }
            state.keywords = keywords.into_iter().collect();
            MessageOutcome::Present(state)
        }
        MessageAssertion::ReplaceMailboxes { mailbox_ids } => {
            state.mailbox_ids = mailbox_ids.clone();
            MessageOutcome::Present(state)
        }
        MessageAssertion::ApplyDiff { diff } => {
            let mut keywords: BTreeSet<String> = state.keywords.into_iter().collect();
            for keyword in &diff.keywords.added {
                keywords.insert(keyword.clone());
            }
            for keyword in &diff.keywords.removed {
                keywords.remove(keyword);
            }
            state.keywords = keywords.into_iter().collect();
            let mut mailbox_ids: BTreeSet<String> = state.mailbox_ids.into_iter().collect();
            for mailbox in &diff.mailboxes.added {
                mailbox_ids.insert(mailbox.clone());
            }
            for mailbox in &diff.mailboxes.removed {
                mailbox_ids.remove(mailbox);
            }
            state.mailbox_ids = mailbox_ids.into_iter().collect();
            MessageOutcome::Present(state)
        }
        MessageAssertion::Destroy => MessageOutcome::Removed,
    }
}

/// Fold an ordered list of assertions over a confirmed base — the message
/// predictor's `replay(base, pending)` ([replication L1 §5.3](../replication/L1.md)).
/// Returns the optimistic state, or `Removed` if any assertion destroyed it.
pub fn replay_message(base: MessageFoldState, assertions: &[MessageAssertion]) -> MessageOutcome {
    let mut outcome = MessageOutcome::Present(base);
    for assertion in assertions {
        outcome = match outcome {
            MessageOutcome::Present(state) => apply_message_assertion(state, assertion),
            MessageOutcome::Removed => return MessageOutcome::Removed,
        };
    }
    outcome
}

/// Coalesce successive pending assertions for one message into the smallest
/// equivalent sequence ([replication L1 §4.4](../replication/L1.md)): a destroy
/// supersedes everything before it; a later mailbox replace supersedes an
/// earlier one; keyword add/removes merge (a later remove cancels an earlier
/// add of the same keyword and vice versa). The result folds to the same state
/// as the input.
pub fn coalesce_message_assertions(assertions: &[MessageAssertion]) -> Vec<MessageAssertion> {
    // Destroy is terminal: nothing after it matters, nothing before it survives.
    if let Some(position) = assertions
        .iter()
        .position(|assertion| matches!(assertion, MessageAssertion::Destroy))
    {
        let _ = position;
        return vec![MessageAssertion::Destroy];
    }

    // An ApplyDiff carries an add/remove delta for both facets; the coalesce
    // vocabulary (SetKeywords + ReplaceMailboxes) cannot express a mailbox
    // delta without the base, so when one is present we leave the sequence
    // uncoalesced. ApplyDiff is the undo/redo vehicle (rare, transient), and the
    // idempotent fold already makes the uncoalesced result visually correct.
    if assertions
        .iter()
        .any(|assertion| matches!(assertion, MessageAssertion::ApplyDiff { .. }))
    {
        return assertions.to_vec();
    }

    let mut added: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut latest_mailboxes: Option<Vec<String>> = None;
    let mut saw_keywords = false;

    let toggle = |list: &mut Vec<String>, other: &mut Vec<String>, keyword: &str| {
        other.retain(|existing| existing != keyword);
        if !list.iter().any(|existing| existing == keyword) {
            list.push(keyword.to_string());
        }
    };

    for assertion in assertions {
        match assertion {
            MessageAssertion::SetKeywords { add, remove } => {
                saw_keywords = true;
                for keyword in add {
                    toggle(&mut added, &mut removed, keyword);
                }
                for keyword in remove {
                    toggle(&mut removed, &mut added, keyword);
                }
            }
            MessageAssertion::ReplaceMailboxes { mailbox_ids } => {
                latest_mailboxes = Some(mailbox_ids.clone());
            }
            MessageAssertion::Destroy => unreachable!("handled above"),
            MessageAssertion::ApplyDiff { .. } => unreachable!("guarded above"),
        }
    }

    let mut coalesced = Vec::new();
    if saw_keywords && (!added.is_empty() || !removed.is_empty()) {
        coalesced.push(MessageAssertion::SetKeywords {
            add: added,
            remove: removed,
        });
    }
    if let Some(mailbox_ids) = latest_mailboxes {
        coalesced.push(MessageAssertion::ReplaceMailboxes { mailbox_ids });
    }
    coalesced
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

    fn present(outcome: MessageOutcome) -> MessageFoldState {
        match outcome {
            MessageOutcome::Present(state) => state,
            MessageOutcome::Removed => panic!("expected present"),
        }
    }

    #[test]
    fn set_keywords_adds_and_removes_with_stable_order() {
        let result = present(apply_message_assertion(
            state(&["$seen"], &["inbox"]),
            &MessageAssertion::SetKeywords {
                add: vec!["$flagged".into()],
                remove: vec!["$seen".into()],
            },
        ));
        assert_eq!(result.keywords, vec!["$flagged".to_string()]);
        assert_eq!(result.mailbox_ids, vec!["inbox".to_string()]);
    }

    #[test]
    fn set_keywords_is_idempotent() {
        let assertion = MessageAssertion::SetKeywords {
            add: vec!["$flagged".into()],
            remove: vec![],
        };
        let once = present(apply_message_assertion(state(&[], &[]), &assertion));
        let twice = present(apply_message_assertion(once.clone(), &assertion));
        assert_eq!(once, twice);
    }

    #[test]
    fn replace_mailboxes_replaces_membership() {
        let result = present(apply_message_assertion(
            state(&["$seen"], &["inbox"]),
            &MessageAssertion::ReplaceMailboxes {
                mailbox_ids: vec!["archive".into()],
            },
        ));
        assert_eq!(result.mailbox_ids, vec!["archive".to_string()]);
    }

    #[test]
    fn destroy_removes_and_is_terminal() {
        let outcome = replay_message(
            state(&["$seen"], &["inbox"]),
            &[
                MessageAssertion::Destroy,
                MessageAssertion::SetKeywords {
                    add: vec!["$flagged".into()],
                    remove: vec![],
                },
            ],
        );
        assert_eq!(outcome, MessageOutcome::Removed);
    }

    #[test]
    fn replay_folds_in_order() {
        let outcome = replay_message(
            state(&[], &["inbox"]),
            &[
                MessageAssertion::SetKeywords {
                    add: vec!["$seen".into()],
                    remove: vec![],
                },
                MessageAssertion::ReplaceMailboxes {
                    mailbox_ids: vec!["archive".into()],
                },
            ],
        );
        let result = present(outcome);
        assert_eq!(result.keywords, vec!["$seen".to_string()]);
        assert_eq!(result.mailbox_ids, vec!["archive".to_string()]);
    }

    #[test]
    fn coalesce_collapses_to_destroy() {
        let coalesced = coalesce_message_assertions(&[
            MessageAssertion::SetKeywords {
                add: vec!["$seen".into()],
                remove: vec![],
            },
            MessageAssertion::Destroy,
        ]);
        assert_eq!(coalesced, vec![MessageAssertion::Destroy]);
    }

    #[test]
    fn coalesce_keeps_latest_mailbox_replace_and_nets_keyword_toggle() {
        let coalesced = coalesce_message_assertions(&[
            MessageAssertion::ReplaceMailboxes {
                mailbox_ids: vec!["a".into()],
            },
            MessageAssertion::SetKeywords {
                add: vec!["$seen".into()],
                remove: vec![],
            },
            MessageAssertion::SetKeywords {
                add: vec![],
                remove: vec!["$seen".into()],
            },
            MessageAssertion::ReplaceMailboxes {
                mailbox_ids: vec!["b".into()],
            },
        ]);
        // add $seen then remove $seen nets to remove $seen (last write wins per
        // keyword, not a cancellation to nothing); only the latest mailbox
        // replace survives.
        assert_eq!(
            coalesced,
            vec![
                MessageAssertion::SetKeywords {
                    add: vec![],
                    remove: vec!["$seen".into()],
                },
                MessageAssertion::ReplaceMailboxes {
                    mailbox_ids: vec!["b".into()],
                },
            ]
        );
    }

    #[test]
    fn coalesced_sequence_folds_to_same_state() {
        let base = state(&["$seen"], &["inbox"]);
        let assertions = vec![
            MessageAssertion::SetKeywords {
                add: vec!["$flagged".into()],
                remove: vec![],
            },
            MessageAssertion::ReplaceMailboxes {
                mailbox_ids: vec!["archive".into()],
            },
            MessageAssertion::SetKeywords {
                add: vec![],
                remove: vec!["$seen".into()],
            },
        ];
        let direct = replay_message(base.clone(), &assertions);
        let coalesced = replay_message(base, &coalesce_message_assertions(&assertions));
        assert_eq!(direct, coalesced);
    }

    fn diff(
        kw_added: &[&str],
        kw_removed: &[&str],
        mb_added: &[&str],
        mb_removed: &[&str],
    ) -> MessageChangeDiff {
        MessageChangeDiff {
            keywords: KeywordDelta {
                added: kw_added.iter().map(|s| s.to_string()).collect(),
                removed: kw_removed.iter().map(|s| s.to_string()).collect(),
            },
            mailboxes: KeywordDelta {
                added: mb_added.iter().map(|s| s.to_string()).collect(),
                removed: mb_removed.iter().map(|s| s.to_string()).collect(),
            },
        }
    }

    #[test]
    fn diff_inverse_swaps_added_and_removed_for_both_facets() {
        let d = diff(&["$flagged"], &["$seen"], &["archive"], &["inbox"]);
        assert_eq!(
            d.inverse(),
            diff(&["$seen"], &["$flagged"], &["inbox"], &["archive"])
        );
        // inverse is an involution.
        assert_eq!(d.inverse().inverse(), d);
    }

    #[test]
    fn diff_from_before_after_is_the_uniform_delta() {
        let prev = state(&["$seen"], &["inbox", "drafts"]);
        let curr = state(&["$seen", "$flagged"], &["inbox", "archive"]);
        let d = MessageChangeDiff::from_before_after(&prev, &curr);
        assert_eq!(d.keywords.added, vec!["$flagged".to_string()]);
        assert!(d.keywords.removed.is_empty());
        assert_eq!(d.mailboxes.added, vec!["archive".to_string()]);
        assert_eq!(d.mailboxes.removed, vec!["drafts".to_string()]);
        // A no-op change (prev == curr) yields an empty diff.
        assert!(MessageChangeDiff::from_before_after(&prev, &prev).is_empty());
    }

    #[test]
    fn apply_diff_folds_keywords_and_mailboxes_as_deltas() {
        let result = present(apply_message_assertion(
            state(&["$seen"], &["inbox"]),
            &MessageAssertion::ApplyDiff {
                diff: diff(&["$flagged"], &["$seen"], &["archive"], &["inbox"]),
            },
        ));
        assert_eq!(result.keywords, vec!["$flagged".to_string()]);
        assert_eq!(result.mailbox_ids, vec!["archive".to_string()]);
    }

    #[test]
    fn apply_diff_inverse_undoes_the_forward_diff() {
        let base = state(&["$seen"], &["inbox"]);
        let d = diff(&["$flagged"], &[], &["archive"], &["inbox"]);
        // Forward then inverse reconstructs the base (undo semantics).
        let forward = present(apply_message_assertion(
            base.clone(),
            &MessageAssertion::ApplyDiff { diff: d.clone() },
        ));
        let restored = present(apply_message_assertion(
            forward.clone(),
            &MessageAssertion::ApplyDiff { diff: d.inverse() },
        ));
        assert_eq!(restored, base);
    }

    #[test]
    fn apply_diff_is_idempotent() {
        let assertion = MessageAssertion::ApplyDiff {
            diff: diff(&["$flagged"], &[], &["archive"], &[]),
        };
        let once = present(apply_message_assertion(state(&[], &["inbox"]), &assertion));
        let twice = present(apply_message_assertion(once.clone(), &assertion));
        assert_eq!(once, twice);
    }

    #[test]
    fn coalesce_leaves_an_apply_diff_sequence_uncoalesced() {
        let assertions = vec![
            MessageAssertion::SetKeywords {
                add: vec!["$flagged".into()],
                remove: vec![],
            },
            MessageAssertion::ApplyDiff {
                diff: diff(&[], &["$flagged"], &[], &[]),
            },
        ];
        let coalesced = coalesce_message_assertions(&assertions);
        // Uncoalesced: the input is returned as-is (still folds to the same state).
        assert_eq!(coalesced, assertions);
    }
}
