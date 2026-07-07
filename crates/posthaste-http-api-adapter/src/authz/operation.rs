//! Per-operation action derivation for the named-mutation funnel
//! (`POST /runtime/sessions/{session_id}/mutations`).
//!
//! That one route carries EVERY [`MailOperation`] — from `message.setKeywords`
//! to `message.destroy` and `message.send` — so no single static
//! [`Action`](super::Action) can gate it: a static `Tag` both under-gates (a
//! tag-scoped token could destroy or send) and over-blocks (a move-scoped
//! token could not archive). The route is instead marked
//! [`RouteAction::HandlerDerived`](super::RouteAction::HandlerDerived) and the
//! handler enforces [`required_actions`] per parsed operation, BEFORE dispatch.
//!
//! Deny-by-default is structural, twice over:
//! - the `match` below is exhaustive with **no wildcard arm**, so adding a
//!   `MailOperation` variant fails compilation until it is mapped, and
//! - an operation name outside the vocabulary never reaches the mapping at
//!   all: `MutationRequest` deserialization rejects it (serde enum tag), so
//!   there is nothing to "default" permissively.
//!
//! The verb assignments mirror the per-message REST command routes (the
//! route-table precedent): keywords → `tag`, mailbox membership → `move`,
//! destroy → `delete`, draft/send lifecycle → `send`.

use posthaste_contract_core::MailOperation;

use super::Action;

/// The action requirement one mutation operation carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperationActions {
    /// The token must be permitted EVERY listed action (never empty).
    AllOf(Vec<Action>),
    /// The token must be permitted AT LEAST ONE listed action (never empty).
    AnyOf(Vec<Action>),
}

/// Every write verb a message mutation can represent. Used by the `revCursor`
/// mapping below; deliberately excludes `Read` (no write authority), `Manage`
/// (source admin, not message mutation) and `Mint` (issuance right).
const MESSAGE_WRITE_ACTIONS: [Action; 4] =
    [Action::Tag, Action::Move, Action::Delete, Action::Send];

/// Map a parsed mutation operation to the action(s) a caller's token must be
/// permitted for the runtime to dispatch it. Exhaustive over [`MailOperation`]
/// — no wildcard arm — so a new operation cannot ship unmapped (deny-by-default
/// at compile time).
///
/// `applyDiff` derives from what the diff actually carries: a keywords delta
/// needs `tag`, a mailboxes delta needs `move`, and a diff touching both needs
/// BOTH — the derived requirement is exactly the union of the operation's
/// effects, never less. A degenerate EMPTY diff (a no-op, expressible on the
/// wire) fails closed to the full `{tag, move}` union rather than picking a
/// permissive default.
///
/// `revCursor` is the undo/redo cursor assignment: it mutates no message but
/// rewrites the account's synced undo/redo bookkeeping, so it is a write — a
/// read-only token must not move it — yet it accompanies undo/redo of ANY
/// message write (the diffs themselves are separately authorized `applyDiff`
/// ops). It therefore requires at least one message-write verb.
pub(crate) fn required_actions(operation: &MailOperation) -> OperationActions {
    use MailOperation as Op;
    match operation {
        // Keyword state (incl. the `$seen`/`$flagged` toggles and user tags).
        Op::SetKeywords(_) | Op::SetReadState(_) | Op::SetFlaggedState(_) | Op::SetUserTags(_) => {
            OperationActions::AllOf(vec![Action::Tag])
        }
        // Mailbox membership, including role moves; snooze/unsnooze are role
        // moves (to the snooze / inbox mailbox) with a return-time side record.
        Op::MoveToMailbox(_)
        | Op::MoveToRole(_)
        | Op::ReplaceMailboxes(_)
        | Op::AddToMailbox(_)
        | Op::RemoveFromMailbox(_)
        | Op::Snooze(_)
        | Op::Unsnooze(_) => OperationActions::AllOf(vec![Action::Move]),
        // Destroying a message is the destructive verb.
        Op::Destroy(_) => OperationActions::AllOf(vec![Action::Delete]),
        // Draft lifecycle + send: the REST `save-draft`/`delete-draft`/`send`
        // command routes all gate on `Send`, and this funnel matches them.
        Op::DeleteDraft(_) | Op::SaveDraft(_) | Op::Send(_) => {
            OperationActions::AllOf(vec![Action::Send])
        }
        // The undo/redo vehicle: require exactly what the diff does.
        Op::ApplyDiff(args) => {
            let mut needed = Vec::new();
            if !args.diff.keywords.is_empty() {
                needed.push(Action::Tag);
            }
            if !args.diff.mailboxes.is_empty() {
                needed.push(Action::Move);
            }
            if needed.is_empty() {
                // Empty diff: nothing to derive FROM — fail closed to the full
                // union of what an applyDiff could carry.
                needed = vec![Action::Tag, Action::Move];
            }
            OperationActions::AllOf(needed)
        }
        // Undo/redo cursor bookkeeping: a write, usable by any message-writer.
        Op::RevCursor(_) => OperationActions::AnyOf(MESSAGE_WRITE_ACTIONS.to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_contract_core::mutation_args::{
        MessageMailboxMembershipArgs, MessageMoveToMailboxArgs, MessageMoveToRoleArgs,
        MessageReplaceMailboxesArgs, MessageSetFlaggedStateArgs, MessageSetReadStateArgs,
        MessageSetUserTagsArgs, MessageSnoozeArgs, MessageTargetArgs, MessageUnsnoozeArgs,
    };
    use serde_json::json;

    /// Parse an `applyDiff` op from its wire shape (the diff types live in
    /// replica-core, which this crate reaches only through contract-core).
    fn apply_diff_op(diff: serde_json::Value) -> MailOperation {
        serde_json::from_value(json!({
            "name": "message.applyDiff",
            "args": { "sourceId": "acct", "messageId": "m1", "diff": diff }
        }))
        .expect("applyDiff fixture parses")
    }

    fn keyword_diff() -> serde_json::Value {
        json!({ "keywords": { "added": ["$seen"], "removed": [] } })
    }

    fn mailbox_diff() -> serde_json::Value {
        json!({ "mailboxes": { "added": ["mbx-a"], "removed": [] } })
    }

    /// One representative instance per `MailOperation` variant, with its
    /// expected requirement — the reviewable op→Action table. Together with
    /// the wildcard-free `match` in [`required_actions`] (which makes an
    /// unmapped new variant a compile error), this pins the whole vocabulary.
    fn vocabulary() -> Vec<(MailOperation, OperationActions)> {
        let all = |actions: &[Action]| OperationActions::AllOf(actions.to_vec());
        let save_draft: MailOperation = serde_json::from_value(json!({
            "name": "message.saveDraft",
            "args": {
                "sourceId": "acct", "messageId": "d1",
                "request": { "from": null, "to": [], "cc": [], "bcc": [],
                             "subject": "s", "body": "b",
                             "inReplyTo": null, "references": null }
            }
        }))
        .expect("saveDraft fixture parses");
        let send: MailOperation = serde_json::from_value(json!({
            "name": "message.send",
            "args": {
                "sourceId": "acct", "messageId": "d1",
                "request": { "from": null, "to": [], "cc": [], "bcc": [],
                             "subject": "s", "body": "b",
                             "inReplyTo": null, "references": null, "draftId": "d1" }
            }
        }))
        .expect("send fixture parses");
        let rev_cursor: MailOperation = serde_json::from_value(json!({
            "name": "revCursor",
            "args": { "accountId": "acct", "cursorStepId": null, "redoTail": [] }
        }))
        .expect("revCursor fixture parses");
        let set_keywords: MailOperation = serde_json::from_value(json!({
            "name": "message.setKeywords",
            "args": { "sourceId": "acct", "messageId": "m1",
                      "command": { "add": ["$seen"], "remove": [] } }
        }))
        .expect("setKeywords fixture parses");
        assert!(
            matches!(set_keywords, MailOperation::SetKeywords(_)),
            "fixture drifted"
        );
        vec![
            (set_keywords, all(&[Action::Tag])),
            (
                MailOperation::SetReadState(MessageSetReadStateArgs {
                    source_id: "acct".into(),
                    message_id: "m1".into(),
                    read: true,
                }),
                all(&[Action::Tag]),
            ),
            (
                MailOperation::SetFlaggedState(MessageSetFlaggedStateArgs {
                    source_id: "acct".into(),
                    message_id: "m1".into(),
                    flagged: true,
                }),
                all(&[Action::Tag]),
            ),
            (
                MailOperation::SetUserTags(MessageSetUserTagsArgs {
                    source_id: "acct".into(),
                    message_id: "m1".into(),
                    add: vec!["t".into()],
                    remove: vec![],
                }),
                all(&[Action::Tag]),
            ),
            (
                MailOperation::MoveToMailbox(MessageMoveToMailboxArgs {
                    source_id: "acct".into(),
                    message_id: "m1".into(),
                    mailbox_id: "mbx".into(),
                }),
                all(&[Action::Move]),
            ),
            (
                MailOperation::MoveToRole(MessageMoveToRoleArgs {
                    source_id: "acct".into(),
                    message_id: "m1".into(),
                    role: "archive".into(),
                }),
                all(&[Action::Move]),
            ),
            (
                MailOperation::ReplaceMailboxes(MessageReplaceMailboxesArgs {
                    source_id: "acct".into(),
                    message_id: "m1".into(),
                    mailbox_ids: vec!["mbx".into()],
                }),
                all(&[Action::Move]),
            ),
            (
                MailOperation::AddToMailbox(MessageMailboxMembershipArgs {
                    source_id: "acct".into(),
                    message_id: "m1".into(),
                    mailbox_id: "mbx".into(),
                }),
                all(&[Action::Move]),
            ),
            (
                MailOperation::RemoveFromMailbox(MessageMailboxMembershipArgs {
                    source_id: "acct".into(),
                    message_id: "m1".into(),
                    mailbox_id: "mbx".into(),
                }),
                all(&[Action::Move]),
            ),
            (
                MailOperation::Snooze(MessageSnoozeArgs {
                    source_id: "acct".into(),
                    message_id: "m1".into(),
                    until: 1_800_000_000,
                }),
                all(&[Action::Move]),
            ),
            (
                MailOperation::Unsnooze(MessageUnsnoozeArgs {
                    source_id: "acct".into(),
                    message_id: "m1".into(),
                }),
                all(&[Action::Move]),
            ),
            (
                MailOperation::Destroy(MessageTargetArgs {
                    source_id: "acct".into(),
                    message_id: "m1".into(),
                }),
                all(&[Action::Delete]),
            ),
            (
                serde_json::from_value::<MailOperation>(json!({
                    "name": "message.deleteDraft",
                    "args": { "sourceId": "acct", "messageId": "m1", "draftId": "d1" }
                }))
                .expect("deleteDraft fixture parses"),
                all(&[Action::Send]),
            ),
            (save_draft, all(&[Action::Send])),
            (send, all(&[Action::Send])),
            (apply_diff_op(keyword_diff()), all(&[Action::Tag])),
            (
                rev_cursor,
                OperationActions::AnyOf(MESSAGE_WRITE_ACTIONS.to_vec()),
            ),
        ]
    }

    /// The op→Action table: every operation in the vocabulary derives exactly
    /// its documented requirement, and no requirement is ever empty (an empty
    /// requirement would silently degrade to "authenticity only").
    #[test]
    fn every_operation_maps_to_its_documented_actions() {
        let table = vocabulary();
        // One entry per MailOperation variant (16 named ops + revCursor). The
        // wildcard-free match makes a NEW variant a compile error; this count
        // makes a table entry for it a review obligation.
        assert_eq!(table.len(), 17, "one table entry per MailOperation variant");
        for (op, expected) in table {
            let derived = required_actions(&op);
            assert_eq!(
                derived,
                expected,
                "operation {} must require {expected:?}",
                op.name()
            );
            let actions = match &derived {
                OperationActions::AllOf(actions) | OperationActions::AnyOf(actions) => actions,
            };
            assert!(
                !actions.is_empty(),
                "operation {} must require at least one action",
                op.name()
            );
        }
    }

    /// `applyDiff` requires exactly the union of the facets the diff touches,
    /// and an empty diff fails closed to the full union.
    #[test]
    fn apply_diff_requirement_follows_the_diff_facets() {
        assert_eq!(
            required_actions(&apply_diff_op(keyword_diff())),
            OperationActions::AllOf(vec![Action::Tag])
        );
        assert_eq!(
            required_actions(&apply_diff_op(mailbox_diff())),
            OperationActions::AllOf(vec![Action::Move])
        );
        let both = json!({
            "keywords": { "added": ["$seen"], "removed": [] },
            "mailboxes": { "added": ["mbx-a"], "removed": [] }
        });
        assert_eq!(
            required_actions(&apply_diff_op(both)),
            OperationActions::AllOf(vec![Action::Tag, Action::Move])
        );
        assert_eq!(
            required_actions(&apply_diff_op(json!({}))),
            OperationActions::AllOf(vec![Action::Tag, Action::Move]),
            "an empty diff must fail closed to the full union"
        );
    }

    /// An operation name outside the vocabulary is rejected at the parse
    /// boundary — there is no permissive default for it to reach.
    #[test]
    fn unknown_operation_names_fail_deserialization() {
        let result: Result<MailOperation, _> = serde_json::from_value(serde_json::json!({
            "name": "message.futureOp",
            "args": { "sourceId": "acct", "messageId": "m1" }
        }));
        assert!(result.is_err(), "unknown op names must not parse");
    }
}
