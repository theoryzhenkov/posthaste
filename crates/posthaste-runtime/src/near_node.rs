//! The runtime as a near node of the runtime↔backend link.
//!
//! Two pieces ([replication backend-link L2 §5](../replication/backend-link/L2.md)):
//!
//! - [`RuntimeBackendOutbox`] — the runtime's outbox **toward the backend**: the
//!   mutations it has forwarded but the backend has not yet confirmed. A
//!   mutation is accepted here when it is forwarded and retired when its receipt
//!   returns; while it is held, the runtime's served views fold it optimistically.
//!   In the co-located in-process deployment confirmation is synchronous, so the
//!   outbox is empty between mutations and the fold is a pass-through; it holds
//!   work only when the backend is remote or unreachable (optimistic offline).
//!
//! - [`apply_outbox_overlay`] — folds the outbox over a recomputed mail-list view
//!   using the **shared** `posthaste-link-replica::MailListReplica`, the same
//!   replica the browser client runs as WASM (assertion `one-replica`). The
//!   served rows are the base; the outbox is the pending set; the projection is
//!   the optimistic result.
//!
//! @spec docs/replication/backend-link/L2#5-the-runtime-near-node-read-replica-outbox

use std::sync::Mutex;

use posthaste_link_core::{MessageAssertion, MutationId, PendingMessageMutation};
use posthaste_link_replica::{MailListReplica, MailListRow};
use posthaste_runtime_contract::{MailListViewState, MutationRequest};
use serde_json::Value;

use crate::mutation_args::{
    keyword_toggle, MessageApplyDiffArgs, MessageMoveToMailboxArgs, MessageReplaceMailboxesArgs,
    MessageSetFlaggedStateArgs, MessageSetKeywordsMutationArgs, MessageSetReadStateArgs,
    MessageSetUserTagsArgs, MessageTargetArgs,
};

/// The runtime's outbox toward the backend: forwarded-but-unconfirmed message
/// mutations, ordered, idempotent on mutation id.
#[derive(Default)]
pub(crate) struct RuntimeBackendOutbox {
    pending: Mutex<Vec<PendingMessageMutation>>,
}

impl RuntimeBackendOutbox {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Accept a forwarded mutation (idempotent on id).
    pub(crate) fn accept(&self, mutation: PendingMessageMutation) {
        let mut pending = self.pending.lock().expect("outbox lock poisoned");
        if pending.iter().any(|held| held.id == mutation.id) {
            return;
        }
        pending.push(mutation);
    }

    /// Retire a mutation once the backend has confirmed (or failed) it.
    pub(crate) fn retire(&self, id: &MutationId) {
        self.pending
            .lock()
            .expect("outbox lock poisoned")
            .retain(|held| &held.id != id);
    }

    /// A snapshot of the pending mutations, for folding into a recompute.
    pub(crate) fn snapshot(&self) -> Vec<PendingMessageMutation> {
        self.pending.lock().expect("outbox lock poisoned").clone()
    }
}

/// The optimistic message effect of a named mutation, for the outbox, plus the
/// message it targets. `None` for mutations whose effect the runtime cannot form
/// from the request alone — role moves (archive/trash/moveToRole) need the
/// account's role→mailbox resolution, so they are not folded optimistically yet
/// and simply forward.
pub(crate) fn named_message_assertion(
    request: &MutationRequest,
) -> Option<(String, MessageAssertion)> {
    match request.name.as_str() {
        "message.setKeywords" => {
            let args: MessageSetKeywordsMutationArgs = parse(request)?;
            Some((
                args.message_id,
                MessageAssertion::SetKeywords {
                    add: args.command.add,
                    remove: args.command.remove,
                },
            ))
        }
        "message.setReadState" => {
            let args: MessageSetReadStateArgs = parse(request)?;
            let command = keyword_toggle("$seen", args.read);
            Some((
                args.message_id,
                MessageAssertion::SetKeywords {
                    add: command.add,
                    remove: command.remove,
                },
            ))
        }
        "message.setFlaggedState" => {
            let args: MessageSetFlaggedStateArgs = parse(request)?;
            let command = keyword_toggle("$flagged", args.flagged);
            Some((
                args.message_id,
                MessageAssertion::SetKeywords {
                    add: command.add,
                    remove: command.remove,
                },
            ))
        }
        "message.setUserTags" => {
            let args: MessageSetUserTagsArgs = parse(request)?;
            Some((
                args.message_id,
                MessageAssertion::SetKeywords {
                    add: args.add,
                    remove: args.remove,
                },
            ))
        }
        "message.moveToMailbox" => {
            let args: MessageMoveToMailboxArgs = parse(request)?;
            Some((
                args.message_id,
                MessageAssertion::ReplaceMailboxes {
                    mailbox_ids: vec![args.mailbox_id],
                },
            ))
        }
        "message.replaceMailboxes" => {
            let args: MessageReplaceMailboxesArgs = parse(request)?;
            Some((
                args.message_id,
                MessageAssertion::ReplaceMailboxes {
                    mailbox_ids: args.mailbox_ids,
                },
            ))
        }
        "message.destroy" => {
            let args: MessageTargetArgs = parse(request)?;
            Some((args.message_id, MessageAssertion::Destroy))
        }
        "message.applyDiff" => {
            let args: MessageApplyDiffArgs = parse(request)?;
            Some((
                args.message_id,
                MessageAssertion::ApplyDiff { diff: args.diff },
            ))
        }
        _ => None,
    }
}

fn parse<T: for<'de> serde::Deserialize<'de>>(request: &MutationRequest) -> Option<T> {
    serde_json::from_value(request.args.clone()).ok()
}

/// Fold the runtime→backend outbox over a recomputed mail-list view, in place,
/// using the shared replica. Behavior-preserving when the outbox is empty (the
/// in-process default): a short-circuit that leaves the served rows untouched.
///
/// The served rows are the confirmed base; each pending mutation is folded over
/// them; destroyed rows drop. Membership beyond destroy (e.g. archived out of a
/// concrete mailbox) is left for the runtime's next recompute to correct
/// (`project_all`), as the client replica does for views it cannot evaluate
/// locally.
pub(crate) fn apply_outbox_overlay(
    state: &mut MailListViewState,
    pending: &[PendingMessageMutation],
) {
    if pending.is_empty() {
        return;
    }
    let mut replica = MailListReplica::new();
    replica.ingest(
        state
            .rows
            .iter()
            .map(|row| MailListRow {
                message_id: row_message_id(row),
                projection: row.projection.clone(),
            })
            .collect(),
    );
    for mutation in pending {
        replica.accept(
            mutation.id.clone(),
            mutation.message_id.clone(),
            mutation.assertion.clone(),
        );
    }
    if !replica.has_pending() {
        return;
    }
    // Re-key the original rows by message id so the projected (folded) rows keep
    // every non-foldable field (row_key, sort_key, order_key, resource_ref).
    let originals: std::collections::HashMap<String, posthaste_runtime_contract::MailListRowState> =
        state
            .rows
            .iter()
            .map(|row| (row_message_id(row), row.clone()))
            .collect();
    state.rows = replica
        .project_all()
        .into_iter()
        .filter_map(|projection| {
            let id = projection.get("id").and_then(Value::as_str)?;
            let mut row = originals.get(id)?.clone();
            row.projection = projection;
            Some(row)
        })
        .collect();
}

/// A mail-list row's message id — the key both the replica and the outbox use.
/// Read from the row's projection (`MessageSummary.id`).
fn row_message_id(row: &posthaste_runtime_contract::MailListRowState) -> String {
    row.projection
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_runtime_contract::{
        MailListAnchorState, MailListContinuation, MailListProjectionKind, MailListRowState,
        RuntimeCoverage, RuntimeCoverageKind,
    };
    use serde_json::json;

    fn row(id: &str, keywords: &[&str], mailboxes: &[&str]) -> MailListRowState {
        MailListRowState {
            row_key: format!("acct:{id}"),
            resource_ref: Some(format!("message:acct:{id}")),
            projection: json!({
                "id": id,
                "subject": "Subject",
                "keywords": keywords,
                "mailboxIds": mailboxes,
                "isRead": keywords.contains(&"$seen"),
                "isFlagged": keywords.contains(&"$flagged"),
            }),
            sort_key: json!(["2026-06-24T00:00:00Z", id]),
            order_key: "00000000".to_string(),
            pending_markers: Vec::new(),
        }
    }

    fn state(rows: Vec<MailListRowState>) -> MailListViewState {
        MailListViewState {
            scope: Value::Null,
            projection_kind: MailListProjectionKind::Message,
            sort: Value::Null,
            window_request: Value::Null,
            rows,
            continuation: MailListContinuation {
                before_cursor: None,
                after_cursor: None,
                has_before: false,
                has_after: false,
            },
            read_watermark: None,
            coverage: RuntimeCoverage {
                kind: RuntimeCoverageKind::Complete,
                details: Value::Null,
            },
            known_total_count: None,
            pending_mutations: Vec::new(),
            anchor: MailListAnchorState::NotRequested,
        }
    }

    fn pending(id: &str, message_id: &str, assertion: MessageAssertion) -> PendingMessageMutation {
        PendingMessageMutation {
            id: MutationId(id.to_string()),
            message_id: message_id.to_string(),
            assertion,
        }
    }

    #[test]
    fn empty_outbox_leaves_rows_untouched() {
        let mut view = state(vec![row("m1", &[], &["inbox"])]);
        let before = serde_json::to_value(&view.rows).unwrap();
        apply_outbox_overlay(&mut view, &[]);
        assert_eq!(serde_json::to_value(&view.rows).unwrap(), before);
    }

    #[test]
    fn pending_flag_shows_optimistically_keeping_other_fields() {
        let mut view = state(vec![
            row("m1", &[], &["inbox"]),
            row("m2", &["$seen"], &["inbox"]),
        ]);
        apply_outbox_overlay(
            &mut view,
            &[pending(
                "op1",
                "m1",
                MessageAssertion::SetKeywords {
                    add: vec!["$flagged".into()],
                    remove: vec![],
                },
            )],
        );
        assert_eq!(view.rows.len(), 2);
        assert_eq!(view.rows[0].projection["isFlagged"], json!(true));
        assert_eq!(view.rows[0].projection["keywords"], json!(["$flagged"]));
        // Non-foldable fields are preserved.
        assert_eq!(view.rows[0].row_key, "acct:m1");
        assert_eq!(view.rows[0].projection["subject"], json!("Subject"));
        // Unaffected row is unchanged.
        assert_eq!(view.rows[1].projection["isRead"], json!(true));
    }

    #[test]
    fn pending_destroy_drops_the_row() {
        let mut view = state(vec![row("m1", &[], &["inbox"]), row("m2", &[], &["inbox"])]);
        apply_outbox_overlay(
            &mut view,
            &[pending("op1", "m1", MessageAssertion::Destroy)],
        );
        let ids: Vec<&str> = view
            .rows
            .iter()
            .map(|row| row.projection["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["m2"]);
    }

    #[test]
    fn outbox_accept_is_idempotent_and_retire_removes() {
        let outbox = RuntimeBackendOutbox::new();
        let mutation = pending("op1", "m1", MessageAssertion::Destroy);
        outbox.accept(mutation.clone());
        outbox.accept(mutation);
        assert_eq!(outbox.snapshot().len(), 1);
        outbox.retire(&MutationId("op1".into()));
        assert!(outbox.snapshot().is_empty());
    }

    #[test]
    fn named_mutation_assertions_cover_the_derivable_set() {
        let request = |name: &str, args: Value| MutationRequest {
            session_id: None,
            name: name.to_string(),
            args,
            client_mutation_id: posthaste_runtime_contract::ClientMutationId::new("c"),
            context: None,
        };
        let (id, assertion) = named_message_assertion(&request(
            "message.setFlaggedState",
            json!({ "sourceId": "acct", "messageId": "m1", "flagged": true }),
        ))
        .unwrap();
        assert_eq!(id, "m1");
        assert_eq!(
            assertion,
            MessageAssertion::SetKeywords {
                add: vec!["$flagged".into()],
                remove: vec![],
            }
        );
        // Role moves are not folded optimistically yet.
        assert!(named_message_assertion(&request(
            "message.archive",
            json!({ "sourceId": "acct", "messageId": "m1" }),
        ))
        .is_none());
        // applyDiff folds as an ApplyDiff assertion carrying the request's diff
        // (the inverse for an undo, the forward for a redo).
        let (id, assertion) = named_message_assertion(&request(
            "message.applyDiff",
            json!({
                "sourceId": "acct",
                "messageId": "m1",
                "diff": {
                    "keywords": { "added": [], "removed": ["$flagged"] },
                    "mailboxes": { "added": [], "removed": [] }
                }
            }),
        ))
        .unwrap();
        assert_eq!(id, "m1");
        let MessageAssertion::ApplyDiff { diff } = assertion else {
            panic!("expected ApplyDiff");
        };
        assert_eq!(diff.keywords.removed, vec!["$flagged".to_string()]);
    }
}
