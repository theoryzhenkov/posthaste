use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use posthaste_link_core::{
    MessageAssertion, MessageFoldState, MessageOutcome, MessageReplica, MutationId,
    SettlementOutcome, SettlementResult,
};

/// One served mail-list row, as the host maps it from the runtime's
/// `MailListRowState`: the message's stable id plus its full presentation
/// projection (a `MessageSummary` JSON). The replica reads keyword/mailbox state
/// out of the projection to seed the predictor and writes the folded state back
/// on `project`, preserving every other field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailListRow {
    pub message_id: String,
    pub projection: Value,
}

/// Working-set replica for a single mail-list view: the served rows are the
/// confirmed base, the outbox holds pending message mutations, and `project`
/// returns the optimistic rows (folded + filtered) the renderer renders.
///
/// Row order follows the served base; folding never reorders (ordering is
/// derived state the runtime owns and re-serves on recompute). A row drops from
/// the optimistic view when its message is destroyed or when an injected
/// membership predicate rejects its folded state (e.g. archived out of the
/// viewed mailbox); everything else is corrected by the next served base.
///
/// @spec docs/replication/client-link/L2#5-working-set-coverage
#[derive(Clone, Debug, Default)]
pub struct MailListReplica {
    engine: MessageReplica,
    /// Served rows in order: (message id, presentation projection). The
    /// foldable state lives in `engine`'s base; this keeps the rest of each row.
    rows: Vec<MailListRow>,
}

impl MailListReplica {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adopt an authoritative served base (a `ViewSnapshot`/`ViewReplace`'s
    /// rows). Replaces the row set and the confirmed message states; pending
    /// mutations are untouched (they re-fold over the new base). This is the
    /// base-replace half of the rebase loop.
    pub fn ingest(&mut self, rows: Vec<MailListRow>) {
        // Swap the whole confirmed base for the served rows — messages that left
        // the window leave the base — but keep the pending outbox so unconfirmed
        // optimism re-folds over the new base (it retires only on settlement).
        // (Resetting the engine here would silently drop pending intent.)
        self.engine.replace_base(rows.iter().map(|row| {
            (
                row.message_id.clone(),
                fold_state_from_projection(&row.projection),
            )
        }));
        self.rows = rows;
    }

    /// Accept an optimistic message mutation into the outbox (idempotent on
    /// mutation id).
    pub fn accept(
        &mut self,
        mutation_id: MutationId,
        message_id: String,
        assertion: MessageAssertion,
    ) {
        self.engine
            .accept(posthaste_link_core::PendingMessageMutation {
                id: mutation_id,
                message_id,
                assertion,
            });
    }

    /// Settle a pending mutation by its terminal outcome (`Confirmed`/`Failed`).
    pub fn settle(
        &mut self,
        mutation_id: &MutationId,
        outcome: SettlementOutcome,
    ) -> SettlementResult {
        self.engine.settle(mutation_id, outcome)
    }

    /// Whether any optimistic mutation is still pending.
    pub fn has_pending(&self) -> bool {
        self.engine.has_pending()
    }

    /// The optimistic rows the renderer renders: each served row with the
    /// outbox folded over it, in served order, dropping destroyed rows and rows
    /// the membership predicate rejects. `membership` answers "does this folded
    /// message still belong in this view?" — for a concrete-mailbox view the
    /// host passes `|state| state.mailbox_ids.contains(&mailbox)`; for views it
    /// cannot evaluate locally it passes `|_| true` and lets the runtime's next
    /// served base correct membership.
    pub fn project(&self, membership: impl Fn(&MessageFoldState) -> bool) -> Vec<Value> {
        let mut out = Vec::with_capacity(self.rows.len());
        for row in &self.rows {
            match self.engine.project(&row.message_id) {
                Some(MessageOutcome::Present(state)) if membership(&state) => {
                    out.push(apply_fold_to_projection(row.projection.clone(), &state));
                }
                _ => {}
            }
        }
        out
    }

    /// Apply a runtime delta to the served base. `order` is a list of row
    /// message ids; when present, rows whose id is absent are dropped and the
    /// remaining rows are reordered. `upserts` replaces any existing row with
    /// the same message id. Pending mutations are preserved and re-fold over
    /// the new base.
    pub fn apply_delta(&mut self, order: Option<Vec<String>>, upserts: Vec<MailListRow>) {
        let upsert_by_id: HashMap<String, MailListRow> = upserts
            .into_iter()
            .map(|row| (row.message_id.clone(), row))
            .collect();
        self.rows = match order {
            Some(order) => order
                .into_iter()
                .filter_map(|id| {
                    upsert_by_id
                        .get(&id)
                        .or_else(|| self.rows.iter().find(|row| row.message_id == id))
                        .cloned()
                })
                .collect(),
            None => self
                .rows
                .iter()
                .map(|row| {
                    upsert_by_id
                        .get(&row.message_id)
                        .cloned()
                        .unwrap_or_else(|| row.clone())
                })
                .collect(),
        };
        self.engine.replace_base(self.rows.iter().map(|row| {
            (
                row.message_id.clone(),
                fold_state_from_projection(&row.projection),
            )
        }));
    }

    /// Convenience projection that drops only destroyed rows (membership always
    /// passes) — for views whose membership the host cannot evaluate locally.
    pub fn project_all(&self) -> Vec<Value> {
        self.project(|_| true)
    }
}

/// Read the foldable canonical state (keywords + mailbox ids) out of a row's
/// presentation projection. Absent/!array fields read as empty.
fn fold_state_from_projection(projection: &Value) -> MessageFoldState {
    MessageFoldState {
        keywords: string_array(projection.get("keywords")),
        mailbox_ids: string_array(projection.get("mailboxIds")),
    }
}

/// Write the folded canonical state back into a presentation projection,
/// re-deriving the read/flag display flags from the keywords and preserving
/// every other field.
fn apply_fold_to_projection(mut projection: Value, state: &MessageFoldState) -> Value {
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
    use serde_json::json;

    fn row(id: &str, keywords: &[&str], mailboxes: &[&str], subject: &str) -> MailListRow {
        MailListRow {
            message_id: id.to_string(),
            projection: json!({
                "id": id,
                "subject": subject,
                "keywords": keywords,
                "mailboxIds": mailboxes,
                "isRead": keywords.contains(&"$seen"),
                "isFlagged": keywords.contains(&"$flagged"),
            }),
        }
    }

    fn ids(rows: &[Value]) -> Vec<String> {
        rows.iter()
            .map(|r| r["id"].as_str().unwrap().to_string())
            .collect()
    }

    fn flag(id: &str) -> (MutationId, String, MessageAssertion) {
        (
            MutationId(format!("op-{id}")),
            id.to_string(),
            MessageAssertion::SetKeywords {
                add: vec!["$flagged".into()],
                remove: vec![],
            },
        )
    }

    #[test]
    fn projects_served_rows_in_order_without_pending() {
        let mut replica = MailListReplica::new();
        replica.ingest(vec![
            row("m1", &[], &["inbox"], "A"),
            row("m2", &["$seen"], &["inbox"], "B"),
        ]);
        assert_eq!(ids(&replica.project_all()), vec!["m1", "m2"]);
    }

    #[test]
    fn optimistic_flag_updates_the_row_in_place() {
        let mut replica = MailListReplica::new();
        replica.ingest(vec![row("m1", &[], &["inbox"], "A")]);
        let (id, message, assertion) = flag("m1");
        replica.accept(id, message, assertion);
        let rows = replica.project_all();
        assert_eq!(rows[0]["isFlagged"], json!(true));
        assert_eq!(rows[0]["keywords"], json!(["$flagged"]));
        assert!(replica.has_pending());
    }

    #[test]
    fn optimistic_archive_drops_the_row_from_a_concrete_mailbox_view() {
        let mut replica = MailListReplica::new();
        replica.ingest(vec![
            row("m1", &[], &["inbox"], "A"),
            row("m2", &[], &["inbox"], "B"),
        ]);
        // Archive m1: replace its mailboxes (inbox -> archive).
        replica.accept(
            MutationId("op1".into()),
            "m1".into(),
            MessageAssertion::ReplaceMailboxes {
                mailbox_ids: vec!["archive".into()],
            },
        );
        // The Inbox view keeps only rows still in "inbox".
        let inbox = replica.project(|state| state.mailbox_ids.iter().any(|m| m == "inbox"));
        assert_eq!(ids(&inbox), vec!["m2"]);
        // Without the membership filter, the row stays (mailboxes updated).
        let unfiltered = replica.project_all();
        assert_eq!(ids(&unfiltered), vec!["m1", "m2"]);
        assert_eq!(unfiltered[0]["mailboxIds"], json!(["archive"]));
    }

    #[test]
    fn optimistic_destroy_drops_the_row_unconditionally() {
        let mut replica = MailListReplica::new();
        replica.ingest(vec![
            row("m1", &[], &["inbox"], "A"),
            row("m2", &[], &["inbox"], "B"),
        ]);
        replica.accept(
            MutationId("op1".into()),
            "m1".into(),
            MessageAssertion::Destroy,
        );
        assert_eq!(ids(&replica.project_all()), vec!["m2"]);
    }

    #[test]
    fn unrelated_base_update_keeps_unconfirmed_optimism() {
        // Regression for C1: a served base update (e.g. a sibling arrival) that
        // does NOT reflect a still-pending mutation must not drop it. The flag
        // re-folds over the new base and stays pending until settled.
        let mut replica = MailListReplica::new();
        replica.ingest(vec![row("m1", &[], &["inbox"], "A")]);
        let (id, message, assertion) = flag("m1");
        replica.accept(id, message, assertion);
        // Runtime re-serves the list (unrelated recompute) WITHOUT the flag.
        replica.ingest(vec![
            row("m1", &[], &["inbox"], "A"),
            row("m2", &[], &["inbox"], "B"),
        ]);
        let rows = replica.project_all();
        assert_eq!(ids(&rows), vec!["m1", "m2"]);
        assert_eq!(rows[0]["isFlagged"], json!(true), "optimism must survive");
        assert!(replica.has_pending());
    }

    #[test]
    fn confirmation_retires_pending_then_base_carries_it() {
        let mut replica = MailListReplica::new();
        replica.ingest(vec![row("m1", &[], &["inbox"], "A")]);
        replica.accept(
            MutationId("op1".into()),
            "m1".into(),
            MessageAssertion::SetKeywords {
                add: vec!["$flagged".into()],
                remove: vec![],
            },
        );
        // Runtime applies it and re-serves the base with the flag, then settles.
        replica.ingest(vec![row("m1", &["$flagged"], &["inbox"], "A")]);
        replica.settle(&MutationId("op1".into()), SettlementOutcome::Confirmed);
        assert!(!replica.has_pending());
        assert_eq!(replica.project_all()[0]["isFlagged"], json!(true));
    }

    #[test]
    fn failed_settlement_reverts_the_row() {
        let mut replica = MailListReplica::new();
        replica.ingest(vec![row("m1", &[], &["inbox"], "A")]);
        let (id, message, assertion) = flag("m1");
        replica.accept(id, message, assertion);
        assert_eq!(replica.project_all()[0]["isFlagged"], json!(true));
        let result = replica.settle(&MutationId("op-m1".into()), SettlementOutcome::Failed);
        assert!(result.reverted);
        assert_eq!(replica.project_all()[0]["isFlagged"], json!(false));
    }

    #[test]
    fn ingest_replaces_the_base_and_drops_windowed_out_rows() {
        let mut replica = MailListReplica::new();
        replica.ingest(vec![
            row("m1", &[], &["inbox"], "A"),
            row("m2", &[], &["inbox"], "B"),
        ]);
        // A later served base no longer includes m1 (scrolled/refiltered).
        replica.ingest(vec![row("m2", &[], &["inbox"], "B")]);
        assert_eq!(ids(&replica.project_all()), vec!["m2"]);
    }

    #[test]
    fn apply_delta_with_order_reorders_and_drops_rows() {
        let mut replica = MailListReplica::new();
        replica.ingest(vec![
            row("m1", &["$seen"], &["inbox"], "A"),
            row("m2", &[], &["inbox"], "B"),
        ]);
        replica.apply_delta(Some(vec!["m2".into(), "m1".into()]), vec![]);
        assert_eq!(ids(&replica.project_all()), vec!["m2", "m1"]);
    }

    #[test]
    fn apply_delta_upsert_replaces_existing_row() {
        let mut replica = MailListReplica::new();
        replica.ingest(vec![row("m1", &["$seen"], &["inbox"], "A")]);
        replica.apply_delta(
            None,
            vec![row("m1", &["$seen", "$flagged"], &["inbox"], "A")],
        );
        let rows = replica.project_all();
        assert_eq!(rows[0]["isFlagged"], json!(true));
    }

    #[test]
    fn apply_delta_preserves_pending_optimism() {
        let mut replica = MailListReplica::new();
        replica.ingest(vec![row("m1", &[], &["inbox"], "A")]);
        let (id, message, assertion) = flag("m1");
        replica.accept(id, message, assertion);
        replica.apply_delta(None, vec![row("m1", &["$seen"], &["inbox"], "A")]);
        let rows = replica.project_all();
        assert_eq!(rows[0]["isFlagged"], json!(true));
        assert!(replica.has_pending());
    }
}
