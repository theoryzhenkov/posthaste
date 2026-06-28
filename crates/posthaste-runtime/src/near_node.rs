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

use std::sync::{Mutex, MutexGuard};

use posthaste_link_contract::message_mutation::MessageMutation;
use posthaste_link_contract::BaseUpdate;
use posthaste_link_core::{
    MessageAssertion, MessageReplica, MutationId, Outcome, PendingMessageMutation,
};
use posthaste_link_replica::{apply_fold_to_projection, fold_state_from_projection};
use posthaste_runtime_contract::{MailListViewState, MutationRequest};
use serde_json::Value;

/// The runtime's outbox toward the backend: forwarded-but-unconfirmed message
/// mutations, ordered, idempotent on mutation id. Backed by the shared
/// [`MessageReplica`] so retirement is **absorption-gated** for the remote seam,
/// consistent with the client tier ([replication backend-link L2 §5](../replication/backend-link/L2.md)).
///
/// Retirement policy (`retire_on_down_channel`):
///
/// - **Co-located** (`false`): the far node applies the effect before the
///   synchronous receipt returns, so a confirmed op is dropped outright on
///   receipt — the next recompute's base already carries it (`colocated-unchanged`).
/// - **Remote** (`true`): the receipt can return **before** the corresponding
///   `message.updated` reaches the read replica, so a confirmed op is only
///   *marked* confirmed on receipt and retired later by absorption, when the
///   down-channel base assertion that carries its effect arrives ([`apply_base`](Self::apply_base)).
///   Retiring on the receipt instead would recompute against a stale base — the
///   revert-then-reapply flicker the client tier's absorption-gated retire
///   already eliminated.
pub(crate) struct RuntimeBackendOutbox {
    engine: Mutex<MessageReplica>,
    retire_on_down_channel: bool,
}

impl RuntimeBackendOutbox {
    /// `retire_on_down_channel` mirrors `drive_down_channel`: a remote backend
    /// (down-channel bridge spawned) gates retirement on the base assertion; a
    /// co-located one retires on receipt.
    pub(crate) fn new(retire_on_down_channel: bool) -> Self {
        Self {
            engine: Mutex::new(MessageReplica::new()),
            retire_on_down_channel,
        }
    }

    fn engine(&self) -> MutexGuard<'_, MessageReplica> {
        self.engine.lock().expect("outbox lock poisoned")
    }

    /// Accept a forwarded mutation (idempotent on id).
    pub(crate) fn accept(&self, mutation: PendingMessageMutation) {
        self.engine().accept(mutation);
    }

    /// Settle a forwarded mutation from its receipt. `confirmed` is the receipt's
    /// `Confirmed` state (a `Failed` verdict, or a transport error, is not). A
    /// confirmed op on the remote seam is held (marked confirmed) until the base
    /// assertion absorbs it; otherwise — a co-located confirm, or any rejection —
    /// it is dropped outright (a rejection changes no state, so the base never
    /// carries it; it is retired by delivering its verdict, mirroring the client
    /// tier's `Failed` settle).
    pub(crate) fn settle_receipt(&self, id: &MutationId, confirmed: bool) {
        let mut engine = self.engine();
        if confirmed && self.retire_on_down_channel {
            engine.mark_confirmed(id);
        } else {
            engine.drop_pending(id);
        }
    }

    /// Apply one down-channel base assertion: rebase the message, then retire any
    /// confirmed pending op the new base now absorbs (the absorption-gated
    /// retire). The shared `retire_absorbed` keeps an unconfirmed op folded
    /// (idempotent — invisible) and a confirmed-but-not-yet-absorbed op pending,
    /// so a receipt that outran this assertion still retires cleanly once the
    /// base carries the effect. Returns the ids it retired.
    pub(crate) fn apply_base(&self, message_id: &str, update: &BaseUpdate) -> Vec<MutationId> {
        let mut engine = self.engine();
        let key = message_id.to_string();
        match update {
            BaseUpdate::Present(state) => engine.set_base(key.clone(), state.clone()),
            BaseUpdate::Removed => engine.remove_base(&key),
        }
        engine.retire_absorbed(&key)
    }

    /// A snapshot of the pending mutations, for folding into a recompute.
    pub(crate) fn snapshot(&self) -> Vec<PendingMessageMutation> {
        self.engine().pending().to_vec()
    }
}

/// The optimistic message effect of a named mutation, for the outbox, plus the
/// message it targets. `None` for mutations whose effect the runtime cannot form
/// from the request alone — role moves (archive/trash/moveToRole) need the
/// account's role→mailbox resolution, so they are not folded optimistically yet
/// and simply forward.
///
/// Delegates to [`posthaste_link_contract::message_mutation::MessageMutation`] so
/// the name→assertion mapping stays in one place.
pub(crate) fn named_message_assertion(
    request: &MutationRequest,
) -> Option<(String, MessageAssertion)> {
    let mutation = MessageMutation::from_request(request).ok()?;
    let message_id = mutation.message_id().to_string();
    mutation
        .to_assertion()
        .map(|assertion| (message_id, assertion))
}

/// Fold the runtime→backend outbox over a recomputed mail-list view, in place,
/// using the shared convergence engine (the same fold the browser client's
/// entity store runs — assertion `one-replica`). Behavior-preserving when the
/// outbox is empty (the in-process default): a short-circuit that leaves the
/// served rows untouched.
///
/// The served rows are the confirmed base; each pending mutation is folded over
/// them; destroyed rows drop. Membership beyond destroy (e.g. archived out of a
/// concrete mailbox) is left for the runtime's next recompute to correct, as the
/// client store does for views it cannot evaluate locally.
pub(crate) fn apply_outbox_overlay(
    state: &mut MailListViewState,
    pending: &[PendingMessageMutation],
) {
    if pending.is_empty() {
        return;
    }
    let mut engine = MessageReplica::new();
    for row in &state.rows {
        engine.set_base(row_message_id(row), fold_state_from_projection(&row.projection));
    }
    for mutation in pending {
        engine.accept(mutation.clone());
    }
    if !engine.has_pending() {
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
    state.rows = state
        .rows
        .iter()
        .filter_map(|row| {
            let id = row_message_id(row);
            match engine.project(&id) {
                Some(Outcome::Present(fold_state)) => {
                    let mut row = originals.get(&id)?.clone();
                    row.projection = apply_fold_to_projection(row.projection.clone(), &fold_state);
                    Some(row)
                }
                // Destroyed (or no base): drop, mirroring `project_all`.
                _ => None,
            }
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
        CoverageRange, MailListAnchorState, MailListContinuation, MailListProjectionKind,
        MailListRowState, RuntimeCoverage,
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
                ranges: vec![CoverageRange { from: None, to: None }],
            },
            known_total_count: None,
            anchor: MailListAnchorState::NotRequested,
        }
    }

    fn pending(id: &str, message_id: &str, assertion: MessageAssertion) -> PendingMessageMutation {
        PendingMessageMutation {
            id: MutationId(id.to_string()),
            key: message_id.to_string(),
            effect: assertion,
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

    fn flag() -> MessageAssertion {
        MessageAssertion::SetKeywords {
            add: vec!["$flagged".into()],
            remove: vec![],
        }
    }

    #[test]
    fn outbox_accept_is_idempotent() {
        let outbox = RuntimeBackendOutbox::new(false);
        let mutation = pending("op1", "m1", MessageAssertion::Destroy);
        outbox.accept(mutation.clone());
        outbox.accept(mutation);
        assert_eq!(outbox.snapshot().len(), 1);
    }

    #[test]
    fn colocated_confirm_retires_on_receipt() {
        // Co-located: the base already carries the effect when the receipt
        // returns, so a confirmed op is dropped outright (`colocated-unchanged`).
        let outbox = RuntimeBackendOutbox::new(false);
        outbox.accept(pending("op1", "m1", flag()));
        outbox.settle_receipt(&MutationId("op1".into()), true);
        assert!(outbox.snapshot().is_empty());
    }

    #[test]
    fn rejection_retires_on_receipt_in_either_mode() {
        // A `Failed` verdict (confirmed == false) is dropped on receipt even on
        // the remote seam: the base never absorbs a rejection.
        for remote in [false, true] {
            let outbox = RuntimeBackendOutbox::new(remote);
            outbox.accept(pending("op1", "m1", flag()));
            outbox.settle_receipt(&MutationId("op1".into()), false);
            assert!(outbox.snapshot().is_empty(), "remote={remote}");
        }
    }

    #[test]
    fn remote_confirm_holds_the_op_until_the_base_assertion_absorbs_it() {
        // The regression guard for the runtime near-node flicker: on the remote
        // seam a confirmed receipt that OUTRUNS the firehose `message.updated`
        // must NOT retire the op — it stays folded so a recompute in that window
        // reads the optimistic (flagged) state, not a stale revert. It retires
        // only once the down-channel base assertion carries the effect.
        let outbox = RuntimeBackendOutbox::new(true);
        outbox.accept(pending("op1", "m1", flag()));

        // Receipt confirms, but the base assertion has not arrived yet.
        outbox.settle_receipt(&MutationId("op1".into()), true);
        let snapshot = outbox.snapshot();
        assert_eq!(snapshot.len(), 1, "op held until absorbed");

        // A recompute in this window folds the still-pending op: the row stays
        // optimistically flagged — no revert (the flicker would show it unflagged
        // here, then re-flag on the firehose).
        let mut view = state(vec![row("m1", &[], &["inbox"])]);
        apply_outbox_overlay(&mut view, &snapshot);
        assert_eq!(view.rows[0].projection["isFlagged"], json!(true));

        // The firehose arrives: the base now carries the flag, so the op is
        // retired by absorption.
        let retired = outbox.apply_base(
            "m1",
            &BaseUpdate::Present(posthaste_link_core::MessageFoldState {
                keywords: vec!["$flagged".into()],
                mailbox_ids: vec!["inbox".into()],
            }),
        );
        assert_eq!(retired, vec![MutationId("op1".into())]);
        assert!(outbox.snapshot().is_empty());

        // Recomputing over the (now flagged) authoritative base with the empty
        // outbox is still flagged — the convergence completed without a flicker.
        let mut view = state(vec![row("m1", &["$flagged"], &["inbox"])]);
        apply_outbox_overlay(&mut view, &outbox.snapshot());
        assert_eq!(view.rows[0].projection["isFlagged"], json!(true));
    }

    #[test]
    fn remote_confirm_keeps_the_op_when_the_base_assertion_does_not_yet_carry_it() {
        // A base assertion that arrives BEFORE the mutation applied (an unrelated
        // re-serve) must not retire the confirmed op: it is not absorbed, so it
        // holds for a later assertion that carries the effect.
        let outbox = RuntimeBackendOutbox::new(true);
        outbox.accept(pending("op1", "m1", flag()));
        outbox.settle_receipt(&MutationId("op1".into()), true);
        let retired = outbox.apply_base(
            "m1",
            &BaseUpdate::Present(posthaste_link_core::MessageFoldState {
                keywords: vec![],
                mailbox_ids: vec!["inbox".into()],
            }),
        );
        assert!(retired.is_empty());
        assert_eq!(outbox.snapshot().len(), 1);
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
        // Role moves aren't folded at the near-node (no role map here) — the
        // client adapter resolves them via `to_assertion_with_roles`.
        assert!(named_message_assertion(&request(
            "message.moveToRole",
            json!({ "sourceId": "acct", "messageId": "m1", "role": "archive" }),
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
