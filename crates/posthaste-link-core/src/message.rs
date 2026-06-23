use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// The subset of canonical message state a mutation's local effect transforms:
/// keyword set and mailbox membership. Renderer-facing derivations (is_read,
/// is_flagged) are computed by the caller from `keywords`; the predictor only
/// moves keywords and membership so it stays independent of presentation shape.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageFoldState {
    pub keywords: Vec<String>,
    pub mailbox_ids: Vec<String>,
}

/// A named mutation's local effect on one message. Keyword changes are an
/// add/remove pair (idempotent: adding a present keyword or removing an absent
/// one is a no-op); mailbox membership is a full desired-state replace; destroy
/// removes the message. These mirror the outbox operation kinds the runtime
/// already enqueues.
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
}

/// Result of folding assertions over a message: its new state, or removed
/// (destroyed). Removal is terminal — later assertions over a removed message
/// are no-ops.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageOutcome {
    Present(MessageFoldState),
    Removed,
}

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
}
