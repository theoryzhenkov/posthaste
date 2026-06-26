//! wasm-bindgen boundary for the client-layer replica.
//!
//! Exposes the portable [`posthaste_link_replica`] view layer to JavaScript so
//! the web `replicaAdapter` ([client-link L2 §6](../replication/client-link/L2.md)) can drive
//! it in the browser. The host (JS) owns transport (fetch/SSE to the remote
//! runtime) and persistence (IndexedDB); this boundary is pure compute over
//! values passed as JSON strings, which keeps the dependency surface to
//! `wasm-bindgen` alone (no `serde-wasm-bindgen`) and the type contract explicit.
//!
//! @spec docs/replication/client-link/L2#3-the-wasm-boundary-posthaste-link-wasm

use serde::Deserialize;
use serde_json::Value;
use wasm_bindgen::prelude::*;

use posthaste_link_core::{MessageAssertion, MutationId, SettlementOutcome};
use posthaste_link_replica::{MailListReplica, MailListRow};

pub mod mutation;
pub mod view_replica;

/// A live mail-list replica owned by JS: the served rows are its base, the
/// outbox holds optimistic mutations, and `projectJson` returns the folded rows.
#[wasm_bindgen]
pub struct MailListReplicaHandle {
    inner: MailListReplica,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcceptArgs {
    mutation_id: String,
    message_id: String,
    assertion: MessageAssertion,
}

#[wasm_bindgen]
impl MailListReplicaHandle {
    #[wasm_bindgen(constructor)]
    pub fn new() -> MailListReplicaHandle {
        MailListReplicaHandle {
            inner: MailListReplica::new(),
        }
    }

    /// Adopt a served base: `rows_json` is a JSON array of `{messageId,
    /// projection}` (the host maps it from the runtime's `MailListViewState`).
    #[wasm_bindgen(js_name = ingestJson)]
    pub fn ingest_json(&mut self, rows_json: &str) -> Result<(), JsError> {
        let rows: Vec<MailListRow> =
            serde_json::from_str(rows_json).map_err(|error| JsError::new(&error.to_string()))?;
        self.inner.ingest(rows);
        Ok(())
    }

    /// Accept an optimistic mutation: `accept_json` is `{mutationId, messageId,
    /// assertion}` where `assertion` is `{kind:"setKeywords",add,remove}` /
    /// `{kind:"replaceMailboxes",mailboxIds}` / `{kind:"destroy"}`.
    #[wasm_bindgen(js_name = acceptJson)]
    pub fn accept_json(&mut self, accept_json: &str) -> Result<(), JsError> {
        let args: AcceptArgs =
            serde_json::from_str(accept_json).map_err(|error| JsError::new(&error.to_string()))?;
        self.inner.accept(
            MutationId(args.mutation_id),
            args.message_id,
            args.assertion,
        );
        Ok(())
    }

    /// Settle a pending mutation. `outcome` is `"confirmed"` or `"failed"`.
    /// Returns `true` when the settlement reverted an optimistic change (a
    /// failure the host should surface).
    #[wasm_bindgen]
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
            .inner
            .settle(&MutationId(mutation_id.to_string()), outcome)
            .reverted)
    }

    #[wasm_bindgen(js_name = hasPending)]
    pub fn has_pending(&self) -> bool {
        self.inner.has_pending()
    }

    /// The optimistic rows as a JSON array of projections, in served order. When
    /// `mailbox_id` is provided, rows whose folded membership no longer includes
    /// it are dropped (concrete-mailbox archive-out); otherwise only destroyed
    /// rows drop and membership is left to the runtime's next served base.
    #[wasm_bindgen(js_name = projectJson)]
    pub fn project_json(&self, mailbox_id: Option<String>) -> Result<String, JsError> {
        let rows: Vec<Value> = match mailbox_id {
            Some(mailbox_id) => self
                .inner
                .project(|state| state.mailbox_ids.iter().any(|id| id == &mailbox_id)),
            None => self.inner.project_all(),
        };
        serde_json::to_string(&rows).map_err(|error| JsError::new(&error.to_string()))
    }
}

impl Default for MailListReplicaHandle {
    fn default() -> Self {
        Self::new()
    }
}
