//! WASM replica backed by the runtime's full `MailListViewState` rows.
//!
//! The `RuntimeMailListReplica` handle owns the confirmed base rows (including
//! non-foldable metadata like `row_key` and `resource_ref`), applies runtime
//! deltas, folds pending mutations, and returns full projected rows. This lets the
//! TypeScript `replicaAdapter` delegate the base+delta+pending fold to Rust
//! instead of reimplementing it in JS.

use std::collections::HashMap;

use serde_json::Value;
use wasm_bindgen::prelude::*;

use posthaste_link_core::{MessageAssertion, MutationId, SettlementOutcome};
use posthaste_link_replica::{MailListReplica, MailListRow};
use posthaste_runtime_contract::{MailListDelta, MailListRowState};

/// A mail-list replica that operates on full runtime view-state rows.
///
/// The host owns transport and persistence; this boundary owns the fold.
#[wasm_bindgen]
pub struct RuntimeMailListReplica {
    engine: MailListReplica,
    /// Full runtime rows in served order, keyed by `row_key` for delta
    /// upserts and by message id for the fold engine.
    rows: Vec<MailListRowState>,
}

#[wasm_bindgen]
impl RuntimeMailListReplica {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            engine: MailListReplica::new(),
            rows: Vec::new(),
        }
    }

    /// Adopt a served `MailListViewState` rows array as the confirmed base.
    /// Replaces the base and drops any rows no longer present, but keeps
    /// pending mutations so they re-fold over the new base.
    #[wasm_bindgen(js_name = ingestViewJson)]
    pub fn ingest_view_json(&mut self, rows_json: &str) -> Result<(), JsError> {
        let rows: Vec<MailListRowState> =
            serde_json::from_str(rows_json).map_err(|error| JsError::new(&error.to_string()))?;
        self.engine
            .ingest(rows.iter().map(row_to_mail_list_row).collect());
        self.rows = rows;
        Ok(())
    }

    /// Apply a runtime `MailListDelta` to the confirmed base.
    ///
    /// When `order` is present, rows whose `row_key` is absent are dropped and
    /// the rest are reordered; `upserts` replace rows by `row_key`. Pending
    /// mutations are preserved and re-folded.
    #[wasm_bindgen(js_name = applyDeltaJson)]
    pub fn apply_delta_json(&mut self, delta_json: &str) -> Result<(), JsError> {
        let delta: MailListDelta =
            serde_json::from_str(delta_json).map_err(|error| JsError::new(&error.to_string()))?;
        let engine_order = engine_order_for_delta(&self.rows, &delta);
        let engine_upserts: Vec<MailListRow> =
            delta.upserts.iter().map(row_to_mail_list_row).collect();
        self.rows = apply_delta_to_rows(&self.rows, &delta);
        self.engine.apply_delta(engine_order, engine_upserts);
        Ok(())
    }

    /// Accept an optimistic mutation by assertion JSON, the same shape used by
    /// `MailListReplicaHandle`.
    #[wasm_bindgen(js_name = acceptJson)]
    pub fn accept_json(&mut self, accept_json: &str) -> Result<(), JsError> {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct AcceptArgs {
            mutation_id: String,
            message_id: String,
            assertion: MessageAssertion,
        }
        let args: AcceptArgs =
            serde_json::from_str(accept_json).map_err(|error| JsError::new(&error.to_string()))?;
        self.engine.accept(
            MutationId(args.mutation_id),
            args.message_id,
            args.assertion,
        );
        Ok(())
    }

    /// Settle a pending mutation. Returns `true` when the settlement reverted
    /// an optimistic change.
    pub fn settle(&mut self, mutation_id: &str, outcome: &str) -> Result<bool, JsError> {
        let outcome = match outcome {
            "confirmed" => SettlementOutcome::Confirmed,
            "failed" => SettlementOutcome::Failed,
            other => {
                return Err(JsError::new(&format!(
                    "unknown settlement outcome: {other}"
                )))
            }
        };
        Ok(self
            .engine
            .settle(&MutationId(mutation_id.to_string()), outcome)
            .reverted)
    }

    #[wasm_bindgen(js_name = hasPending)]
    pub fn has_pending(&self) -> bool {
        self.engine.has_pending()
    }

    /// Return the optimistic rows as a JSON array of full `MailListRowState`.
    /// Pass the viewed concrete `mailbox_id` to drop archive-out rows.
    #[wasm_bindgen(js_name = projectViewJson)]
    pub fn project_view_json(&self, mailbox_id: Option<String>) -> Result<String, JsError> {
        let projections = match mailbox_id {
            Some(mailbox_id) => self
                .engine
                .project(|state| state.mailbox_ids.iter().any(|id| id == &mailbox_id)),
            None => self.engine.project_all(),
        };
        let by_id: HashMap<String, &MailListRowState> = self
            .rows
            .iter()
            .map(|row| (row_message_id(row), row))
            .collect();
        let projected_rows: Vec<MailListRowState> = projections
            .iter()
            .filter_map(|projection| {
                let id = projection.get("id").and_then(Value::as_str)?;
                let original = by_id.get(id)?;
                let mut row = (*original).clone();
                row.projection = projection.clone();
                Some(row)
            })
            .collect();
        serde_json::to_string(&projected_rows).map_err(|error| JsError::new(&error.to_string()))
    }
}

impl Default for RuntimeMailListReplica {
    fn default() -> Self {
        Self::new()
    }
}

fn row_message_id(row: &MailListRowState) -> String {
    if let Some(resource_ref) = row.resource_ref.as_deref() {
        if let Some(stripped) = resource_ref.strip_prefix("message:") {
            if let Some((_, id)) = stripped.rsplit_once(':') {
                return id.to_string();
            }
        }
    }
    row.projection
        .get("id")
        .and_then(Value::as_str)
        .map(String::from)
        .unwrap_or_else(|| row.row_key.clone())
}

fn row_to_mail_list_row(row: &MailListRowState) -> MailListRow {
    MailListRow {
        message_id: row_message_id(row),
        projection: row.projection.clone(),
    }
}

fn apply_delta_to_rows(rows: &[MailListRowState], delta: &MailListDelta) -> Vec<MailListRowState> {
    let upserts_by_row_key: HashMap<String, &MailListRowState> = delta
        .upserts
        .iter()
        .map(|row| (row.row_key.clone(), row))
        .collect();
    match &delta.order {
        Some(order) => order
            .iter()
            .filter_map(|row_key| {
                upserts_by_row_key
                    .get(row_key)
                    .copied()
                    .or_else(|| rows.iter().find(|row| &row.row_key == row_key))
                    .cloned()
            })
            .collect(),
        None => rows
            .iter()
            .map(|row| {
                upserts_by_row_key
                    .get(&row.row_key)
                    .copied()
                    .cloned()
                    .unwrap_or_else(|| row.clone())
            })
            .collect(),
    }
}

fn engine_order_for_delta(rows: &[MailListRowState], delta: &MailListDelta) -> Option<Vec<String>> {
    let order = delta.order.as_ref()?;
    let upserts_by_row_key: HashMap<String, &MailListRowState> = delta
        .upserts
        .iter()
        .map(|row| (row.row_key.clone(), row))
        .collect();
    let held_by_row_key: HashMap<String, &MailListRowState> =
        rows.iter().map(|row| (row.row_key.clone(), row)).collect();
    Some(
        order
            .iter()
            .filter_map(|row_key| {
                let row = upserts_by_row_key
                    .get(row_key)
                    .copied()
                    .or_else(|| held_by_row_key.get(row_key).copied())?;
                Some(row_message_id(row))
            })
            .collect(),
    )
}
