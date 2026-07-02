//! The runtime as a near node of the runtime↔authority-server link.
//!
//! Two pieces ([replication authority-server-link L2 §5](../replication/authority-server-link/L2.md)):
//!
//! - [`AuthorityServerPendingSet`] — the runtime's pending set **toward the authority server**: the
//!   mutations it has forwarded but the authority server has not yet confirmed. A
//!   mutation is accepted here when it is forwarded and retired when its receipt
//!   returns; while it is held, the runtime's served views fold it optimistically.
//!   In the co-located in-process deployment confirmation is synchronous, so the
//!   pending set is empty between mutations and the fold is a pass-through; it holds
//!   work only when the authority server is remote or unreachable (optimistic offline).
//!
//! - [`apply_pending_set_overlay`] — folds the pending set over a recomputed mail-list view
//!   using the **shared** convergence kernel and per-entity optimism read
//!   (`posthaste-replica-projector`'s `project_optimistic`), the same fold the
//!   browser client's entity store runs as WASM (assertion `one-replica`). The
//!   served rows are the base; the runtime's pending set folds over them; the
//!   projection is the optimistic result.
//!
//! @spec docs/replication/authority-server-link/L2#5-the-runtime-near-node-read-replica-pending-set

use std::sync::{Mutex, MutexGuard};

use posthaste_authority_server_link::BaseUpdate;
use posthaste_replica_core::{MessageAssertion, MessageReplica, MutationId, PendingMessageMutation};
use posthaste_replica_projector::{fold_state_from_projection, project_optimistic};
use posthaste_contract_core::{MailListViewState, MutationRequest};
use serde_json::Value;

/// The runtime's pending set toward the authority server: forwarded-but-unconfirmed message
/// mutations, ordered, idempotent on mutation id. Composes the shared
/// [`MessageReplica`] — replica-core's `OptimisticReplica` kernel, the same
/// accept-pending / fold-on-read / retire-on-absorption mechanism the client's
/// `EntityStore` mounts (`one-replica-both-seams`, RFC D34/D35a) — so retirement
/// is **absorption-gated** for the remote seam, consistent with the client tier
/// ([replication authority-server-link L2 §5](../replication/authority-server-link/L2.md)).
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
pub(crate) struct AuthorityServerPendingSet {
    engine: Mutex<MessageReplica>,
    retire_on_down_channel: bool,
}

impl AuthorityServerPendingSet {
    /// `retire_on_down_channel` mirrors `drive_down_channel`: a remote authority server
    /// (down-channel bridge spawned) gates retirement on the base assertion; a
    /// co-located one retires on receipt.
    pub(crate) fn new(retire_on_down_channel: bool) -> Self {
        Self {
            engine: Mutex::new(MessageReplica::new()),
            retire_on_down_channel,
        }
    }

    fn engine(&self) -> MutexGuard<'_, MessageReplica> {
        self.engine.lock().expect("pending set lock poisoned")
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

/// The optimistic message effect of an operation, for the pending set, plus the
/// message it targets. `None` for operations whose effect the runtime cannot
/// form from the request alone — role moves (archive/trash/moveToRole) need the
/// account's role→mailbox resolution, so they are not folded optimistically yet
/// and simply forward — and for control operations (`revCursor`) that target no
/// message.
///
/// Delegates to [`MailOperation::fold_effect`] — the single local-effect
/// projection (D34 (b)) the wasm client's optimistic fold also consumes, so the
/// two derivations cannot drift.
pub(crate) fn named_message_assertion(
    request: &MutationRequest,
) -> Option<(String, MessageAssertion)> {
    let message_id = request.operation.message_id()?.to_string();
    request
        .operation
        .fold_effect()
        .map(|assertion| (message_id, assertion))
}

/// Fold the runtime→authority server pending set over a recomputed mail-list view, in place,
/// using the shared convergence engine (the same fold the browser client's
/// entity store runs — assertion `one-replica`). Behavior-preserving when the
/// pending set is empty (the in-process default): a short-circuit that leaves the
/// served rows untouched.
///
/// The served rows are the confirmed base; each pending mutation is folded over
/// them; destroyed rows drop. Membership beyond destroy (e.g. archived out of a
/// concrete mailbox) is left for the runtime's next recompute to correct, as the
/// client store does for views it cannot evaluate locally.
pub(crate) fn apply_pending_set_overlay(
    state: &mut MailListViewState,
    pending: &[PendingMessageMutation],
) {
    if pending.is_empty() {
        return;
    }
    let mut engine = MessageReplica::new();
    for row in &state.rows {
        engine.set_base(
            row_message_id(row),
            fold_state_from_projection(&row.projection),
        );
    }
    for mutation in pending {
        engine.accept(mutation.clone());
    }
    if !engine.has_pending() {
        return;
    }
    // Re-key the original rows by message id so the projected (folded) rows keep
    // every non-foldable field (row_key, sort_key, order_key, resource_ref).
    let originals: std::collections::HashMap<String, posthaste_contract_core::MailListRowState> =
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
            let original = originals.get(&id)?;
            // The shared per-entity optimism read (one projector, RFC D38):
            // destroyed (or no base) folds to None and the row drops,
            // mirroring `project_all`.
            let projection = project_optimistic(&engine, &id, &original.projection)?;
            let mut row = original.clone();
            row.projection = projection;
            Some(row)
        })
        .collect();
}

/// A mail-list row's message id — the key both the replica and the pending set use.
/// Read from the row's projection (`MessageSummary.id`).
fn row_message_id(row: &posthaste_contract_core::MailListRowState) -> String {
    row.projection
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_contract_core::{
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
                ranges: vec![CoverageRange {
                    from: None,
                    to: None,
                }],
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
    fn empty_pending_set_leaves_rows_untouched() {
        let mut view = state(vec![row("m1", &[], &["inbox"])]);
        let before = serde_json::to_value(&view.rows).unwrap();
        apply_pending_set_overlay(&mut view, &[]);
        assert_eq!(serde_json::to_value(&view.rows).unwrap(), before);
    }

    #[test]
    fn pending_flag_shows_optimistically_keeping_other_fields() {
        let mut view = state(vec![
            row("m1", &[], &["inbox"]),
            row("m2", &["$seen"], &["inbox"]),
        ]);
        apply_pending_set_overlay(
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
        apply_pending_set_overlay(
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
    fn the_composed_engine_is_the_shared_optimistic_replica_seam() {
        // Compile-time proof of composition (RFC D35a/D36): the pending set's engine
        // is replica-core's `OptimisticReplica` kernel — the same seam the client
        // `EntityStore` mounts — not a runtime-grown sibling.
        use posthaste_replica_core::{MessageConvergence, OptimisticReplica};
        fn assert_kernel<R: OptimisticReplica<MessageConvergence>>() {}
        assert_kernel::<MessageReplica>();
    }

    #[test]
    fn pending_set_accept_is_idempotent() {
        let pending_set = AuthorityServerPendingSet::new(false);
        let mutation = pending("op1", "m1", MessageAssertion::Destroy);
        pending_set.accept(mutation.clone());
        pending_set.accept(mutation);
        assert_eq!(pending_set.snapshot().len(), 1);
    }

    #[test]
    fn colocated_confirm_retires_on_receipt() {
        // Co-located: the base already carries the effect when the receipt
        // returns, so a confirmed op is dropped outright (`colocated-unchanged`).
        let pending_set = AuthorityServerPendingSet::new(false);
        pending_set.accept(pending("op1", "m1", flag()));
        pending_set.settle_receipt(&MutationId("op1".into()), true);
        assert!(pending_set.snapshot().is_empty());
    }

    #[test]
    fn rejection_retires_on_receipt_in_either_mode() {
        // A `Failed` verdict (confirmed == false) is dropped on receipt even on
        // the remote seam: the base never absorbs a rejection.
        for remote in [false, true] {
            let pending_set = AuthorityServerPendingSet::new(remote);
            pending_set.accept(pending("op1", "m1", flag()));
            pending_set.settle_receipt(&MutationId("op1".into()), false);
            assert!(pending_set.snapshot().is_empty(), "remote={remote}");
        }
    }

    #[test]
    fn remote_confirm_holds_the_op_until_the_base_assertion_absorbs_it() {
        // The regression guard for the runtime near-node flicker: on the remote
        // seam a confirmed receipt that OUTRUNS the firehose `message.updated`
        // must NOT retire the op — it stays folded so a recompute in that window
        // reads the optimistic (flagged) state, not a stale revert. It retires
        // only once the down-channel base assertion carries the effect.
        let pending_set = AuthorityServerPendingSet::new(true);
        pending_set.accept(pending("op1", "m1", flag()));

        // Receipt confirms, but the base assertion has not arrived yet.
        pending_set.settle_receipt(&MutationId("op1".into()), true);
        let snapshot = pending_set.snapshot();
        assert_eq!(snapshot.len(), 1, "op held until absorbed");

        // A recompute in this window folds the still-pending op: the row stays
        // optimistically flagged — no revert (the flicker would show it unflagged
        // here, then re-flag on the firehose).
        let mut view = state(vec![row("m1", &[], &["inbox"])]);
        apply_pending_set_overlay(&mut view, &snapshot);
        assert_eq!(view.rows[0].projection["isFlagged"], json!(true));

        // The firehose arrives: the base now carries the flag, so the op is
        // retired by absorption.
        let retired = pending_set.apply_base(
            "m1",
            &BaseUpdate::Present(posthaste_replica_core::MessageFoldState {
                keywords: vec!["$flagged".into()],
                mailbox_ids: vec!["inbox".into()],
            }),
        );
        assert_eq!(retired, vec![MutationId("op1".into())]);
        assert!(pending_set.snapshot().is_empty());

        // Recomputing over the (now flagged) authoritative base with the empty
        // pending set is still flagged — the convergence completed without a flicker.
        let mut view = state(vec![row("m1", &["$flagged"], &["inbox"])]);
        apply_pending_set_overlay(&mut view, &pending_set.snapshot());
        assert_eq!(view.rows[0].projection["isFlagged"], json!(true));
    }

    #[test]
    fn remote_confirm_keeps_the_op_when_the_base_assertion_does_not_yet_carry_it() {
        // A base assertion that arrives BEFORE the mutation applied (an unrelated
        // re-serve) must not retire the confirmed op: it is not absorbed, so it
        // holds for a later assertion that carries the effect.
        let pending_set = AuthorityServerPendingSet::new(true);
        pending_set.accept(pending("op1", "m1", flag()));
        pending_set.settle_receipt(&MutationId("op1".into()), true);
        let retired = pending_set.apply_base(
            "m1",
            &BaseUpdate::Present(posthaste_replica_core::MessageFoldState {
                keywords: vec![],
                mailbox_ids: vec!["inbox".into()],
            }),
        );
        assert!(retired.is_empty());
        assert_eq!(pending_set.snapshot().len(), 1);
    }

    #[test]
    fn named_mutation_assertions_cover_the_derivable_set() {
        let request = |name: &str, args: Value| -> MutationRequest {
            serde_json::from_value(json!({
                "name": name,
                "args": args,
                "clientMutationId": "c",
            }))
            .expect("request builds from the flat wire shape")
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
        // client adapter resolves them via `fold_effect_with_roles`.
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
