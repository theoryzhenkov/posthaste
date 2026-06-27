//! wasm-bindgen boundary for the reactive [`EntityStore`].
//!
//! The web `entityStoreAdapter` (client-link L2 §6, slice 2e) drives the
//! normalized, keyed entity store from JavaScript. As with
//! [`crate::MailListReplicaHandle`], values cross the boundary as JSON strings
//! (no `serde-wasm-bindgen`), keeping the dependency surface to `wasm-bindgen`
//! alone. The host (JS) owns transport + persistence; this boundary is pure
//! compute over the store's [`EntityStore`] API: register/subscribe by key,
//! ingest authoritative batches, accept/settle optimism, and drain the dirty
//! keys for reactive fan-out.
//!
//! ## Wire contract
//!
//! The JSON shapes are pinned by `entity_store::tests::*_round_trips_*` in
//! `posthaste-link-replica` (camelCase, externally-tagged enums). The host
//! builds these shapes; a mismatch fails the `serde_json` deserialize and
//! surfaces as a `JsError`.
//!
//! @spec docs/eph/DESIGN-L2-client-link-reactive-store
//! @spec docs/eph/PLAN-L2-client-link-reactive-store (2e)

use serde::Deserialize;
use serde_json::Value;
use wasm_bindgen::prelude::*;

use posthaste_link_core::{MessageAssertion, MutationId, SettlementOutcome};
use posthaste_link_replica::{
    EntityStore, SortDirection, SortKey, StoreUpdate, ViewPredicate, ViewRow,
};

/// A live reactive entity store owned by JS: messages, mailboxes (count
/// scalars), and views (ordered row lists + coverage), with a message optimism
/// fold. The host feeds it authoritative batches and reads the dirty keys to
/// drive the renderer.
#[wasm_bindgen]
pub struct EntityStoreHandle {
    inner: EntityStore,
}

/// Args for [`EntityStoreHandle::register_view_json`]: the view's predicate,
/// sort, and initial coverage watermark.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterViewArgs {
    predicate: ViewPredicate,
    sort_field: String,
    sort_direction: SortDirection,
    watermark: Option<SortKey>,
}

/// Args for [`EntityStoreHandle::accept_mutation_json`]: `{mutationId,
/// messageId, assertion}` where `assertion` mirrors the replica predictor's
/// vocabulary (`setKeywords`/`replaceMailboxes`/`destroy`/`applyDiff`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcceptArgs {
    mutation_id: String,
    message_id: String,
    assertion: MessageAssertion,
}

#[wasm_bindgen]
impl EntityStoreHandle {
    #[wasm_bindgen(constructor)]
    pub fn new() -> EntityStoreHandle {
        EntityStoreHandle {
            inner: EntityStore::new(),
        }
    }

    /// Register a view with its predicate, sort, and initial coverage
    /// watermark. The host calls this when a view is opened (or its window
    /// grows). `args_json` is `{predicate, sortField, sortDirection,
    /// watermark?}` where `predicate` is `{"inMailbox":id}` / `"all"` /
    /// `"deferred"` and `watermark` is `{"receivedAt","messageId"}` or null
    /// (reaches BOTTOM). Marks the view dirty.
    #[wasm_bindgen(js_name = registerViewJson)]
    pub fn register_view_json(&mut self, view_id: &str, args_json: &str) -> Result<(), JsError> {
        let args: RegisterViewArgs =
            serde_json::from_str(args_json).map_err(|e| JsError::new(&e.to_string()))?;
        self.inner.register_view(
            view_id,
            args.predicate,
            args.sort_field,
            args.sort_direction,
            args.watermark,
        );
        Ok(())
    }

    /// Replace a view's held rows + watermark (a served snapshot / page /
    /// resync). `rows_json` is a JSON array of `{rowKey, messageId,
    /// sortKey:{receivedAt,messageId}}`; `watermark_json` is the new watermark
    /// (`{"receivedAt","messageId"}` or `null`). Does not touch the message
    /// base — the host ingests the rows' projections atomically in the same
    /// batch via [`ingest_batch_json`](Self::ingest_batch_json) (P1: a row
    /// implies a live base).
    #[wasm_bindgen(js_name = setViewRowsJson)]
    pub fn set_view_rows_json(
        &mut self,
        view_id: &str,
        rows_json: &str,
        watermark_json: &str,
    ) -> Result<(), JsError> {
        let rows: Vec<ViewRow> =
            serde_json::from_str(rows_json).map_err(|e| JsError::new(&e.to_string()))?;
        let watermark: Option<SortKey> =
            serde_json::from_str(watermark_json).map_err(|e| JsError::new(&e.to_string()))?;
        self.inner.set_view_rows(view_id, rows, watermark);
        Ok(())
    }

    /// Close a view (it was closed on the host).
    #[wasm_bindgen(js_name = closeView)]
    pub fn close_view(&mut self, view_id: &str) {
        self.inner.close_view(view_id);
    }

    /// Apply an authoritative batch atomically: every update is applied before
    /// any dirty key is reported. `batch_json` is a JSON array of
    /// `{"message":{messageId, projection, deleted, countDeltas:[{mailboxId,
    /// unreadCount, totalCount}]}}` and/or `{"mailboxCount":{mailboxId,
    /// unreadCount, totalCount}}`.
    #[wasm_bindgen(js_name = ingestBatchJson)]
    pub fn ingest_batch_json(&mut self, batch_json: &str) -> Result<(), JsError> {
        let updates: Vec<StoreUpdate> =
            serde_json::from_str(batch_json).map_err(|e| JsError::new(&e.to_string()))?;
        self.inner.ingest_batch(updates);
        Ok(())
    }

    /// Accept an optimistic message mutation into the outbox (idempotent on
    /// mutation id). `accept_json` is `{mutationId, messageId, assertion}`.
    /// The projected state is re-derived so reads reflect the fold
    /// immediately; a mutation on a not-yet-ingested message is tracked but
    /// deferred.
    #[wasm_bindgen(js_name = acceptMutationJson)]
    pub fn accept_mutation_json(&mut self, accept_json: &str) -> Result<(), JsError> {
        let args: AcceptArgs =
            serde_json::from_str(accept_json).map_err(|e| JsError::new(&e.to_string()))?;
        self.inner
            .accept_mutation(MutationId(args.mutation_id), &args.message_id, args.assertion);
        Ok(())
    }

    /// Settle a pending mutation by its terminal outcome. `outcome` is
    /// `"confirmed"` or `"failed"`. Returns `true` when the settlement reverted
    /// an optimistic change (a failure the host should surface).
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

    /// Whether any optimistic mutation is still pending.
    #[wasm_bindgen(js_name = hasPending)]
    pub fn has_pending(&self) -> bool {
        self.inner.has_pending()
    }

    /// A message's optimistic projection as a JSON string, or `"null"` if the
    /// message is not held or has been optimistically destroyed. The projection
    /// is the confirmed base with the pending outbox folded over it (keywords +
    /// mailbox membership) — never stored as truth.
    #[wasm_bindgen(js_name = messageJson)]
    pub fn message_json(&self, message_id: &str) -> String {
        let projection: Option<Value> = self.inner.message(message_id);
        serde_json::to_string(&projection).unwrap_or_else(|_| "null".to_string())
    }

    /// A mailbox's server-authoritative counts as `{"unreadCount",
    /// "totalCount"}`, or `"null"` if the mailbox is not held.
    #[wasm_bindgen(js_name = mailboxJson)]
    pub fn mailbox_json(&self, mailbox_id: &str) -> String {
        let counts = self.inner.mailbox(mailbox_id);
        serde_json::to_string(&counts).unwrap_or_else(|_| "null".to_string())
    }

    /// A view's rows as a JSON array of `{rowKey, messageId, sortKey}`, or
    /// `"null"` if the view is not registered.
    #[wasm_bindgen(js_name = viewRowsJson)]
    pub fn view_rows_json(&self, view_id: &str) -> String {
        let rows = self.inner.view_rows(view_id);
        serde_json::to_string(&rows).unwrap_or_else(|_| "null".to_string())
    }

    /// A view's **projected** rows as a JSON array of `{rowKey, messageId,
    /// sortKey, projection}` — the optimistic message projection joined to each
    /// row in one call, so the host re-projects a view with a single round-trip
    /// per drain (P1: a row implies a live base, so `projection` is non-null for
    /// every placed row). `"null"` if the view is not registered.
    #[wasm_bindgen(js_name = projectViewJson)]
    pub fn project_view_json(&self, view_id: &str) -> String {
        let rows = match self.inner.view_rows(view_id) {
            Some(r) => r,
            None => return "null".to_string(),
        };
        let projected: Vec<Value> = rows
            .iter()
            .map(|row| {
                let projection = self.inner.message(&row.message_id);
                serde_json::json!({
                    "rowKey": row.row_key,
                    "messageId": row.message_id,
                    "sortKey": row.sort_key,
                    "projection": projection,
                })
            })
            .collect();
        serde_json::to_string(&projected).unwrap_or_else(|_| "[]".to_string())
    }

    /// Drain the keys changed since the last drain as a JSON array of
    /// `{"message":id}` / `{"mailbox":id}` / `{"view":id}`. The host re-reads
    /// these (re-project views, re-write counts). One drain per batch.
    #[wasm_bindgen(js_name = drainDirtyJson)]
    pub fn drain_dirty_json(&mut self) -> String {
        let dirty = self.inner.drain_dirty();
        serde_json::to_string(&dirty).unwrap_or_else(|_| "[]".to_string())
    }
}

impl Default for EntityStoreHandle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Drive the handle end-to-end through the JSON boundary: register a view,
    /// place its rows, ingest an authoritative message batch (projection +
    /// count delta), drain dirty, and read the projected message + view rows +
    /// mailbox counts. Pins the wiring (the serde shapes themselves are pinned
    /// in `posthaste-link-replica`).
    #[test]
    fn handle_round_trips_an_authoritative_batch() {
        let mut handle = EntityStoreHandle::new();
        handle
            .register_view_json(
                "inbox",
                &json!({
                    "predicate": {"inMailbox": "inbox"},
                    "sortField": "receivedAt",
                    "sortDirection": "desc",
                    "watermark": null
                })
                .to_string(),
            )
            .unwrap();
        handle
            .set_view_rows_json(
                "inbox",
                &json!([{
                    "rowKey": "primary:m1",
                    "messageId": "m1",
                    "sortKey": {"receivedAt": "2026-04-29T10:00:00Z", "messageId": "m1"}
                }])
                .to_string(),
                "null",
            )
            .unwrap();

        // An authoritative message batch: the projection + the inbox count delta.
        handle
            .ingest_batch_json(
                &json!([{
                    "message": {
                        "messageId": "m1",
                        "projection": {
                            "id": "m1",
                            "sourceId": "primary",
                            "receivedAt": "2026-04-29T10:00:00Z",
                            "mailboxIds": ["inbox"],
                            "keywords": [],
                            "isRead": false,
                            "isFlagged": false,
                            "subject": "m1"
                        },
                        "deleted": false,
                        "countDeltas": [
                            {"mailboxId": "inbox", "unreadCount": 1, "totalCount": 1}
                        ]
                    }
                }])
                .to_string(),
            )
            .unwrap();

        // The dirty drain reports the message, the mailbox, and the view.
        let dirty: Vec<serde_json::Value> =
            serde_json::from_str(&handle.drain_dirty_json()).unwrap();
        assert!(dirty.contains(&json!({"message": "m1"})));
        assert!(dirty.contains(&json!({"mailbox": "inbox"})));
        assert!(dirty.contains(&json!({"view": "inbox"})));

        // The projected message + view row + mailbox counts read back.
        let msg: serde_json::Value =
            serde_json::from_str(&handle.message_json("m1")).unwrap();
        assert_eq!(msg["subject"], json!("m1"));
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&handle.view_rows_json("inbox")).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["messageId"], json!("m1"));
        let counts: serde_json::Value =
            serde_json::from_str(&handle.mailbox_json("inbox")).unwrap();
        assert_eq!(counts["unreadCount"], json!(1));
        assert_eq!(counts["totalCount"], json!(1));
    }

    /// An optimistic flag folds into the projected message + stays pending.
    #[test]
    fn handle_folds_optimism_and_settles() {
        let mut handle = EntityStoreHandle::new();
        handle
            .register_view_json(
                "inbox",
                &json!({
                    "predicate": {"inMailbox": "inbox"},
                    "sortField": "receivedAt",
                    "sortDirection": "desc",
                    "watermark": null
                })
                .to_string(),
            )
            .unwrap();
        handle
            .ingest_batch_json(
                &json!([{
                    "message": {
                        "messageId": "m1",
                        "projection": {
                            "id": "m1", "sourceId": "primary",
                            "receivedAt": "2026-04-29T10:00:00Z",
                            "mailboxIds": ["inbox"], "keywords": [],
                            "isRead": false, "isFlagged": false, "subject": "m1"
                        },
                        "deleted": false, "countDeltas": []
                    }
                }])
                .to_string(),
            )
            .unwrap();
        handle.drain_dirty_json();

        assert!(!handle.has_pending());
        handle
            .accept_mutation_json(
                &json!({
                    "mutationId": "op1",
                    "messageId": "m1",
                    "assertion": {"kind": "setKeywords", "add": ["$flagged"], "remove": []}
                })
                .to_string(),
            )
            .unwrap();
        assert!(handle.has_pending());
        let msg: serde_json::Value =
            serde_json::from_str(&handle.message_json("m1")).unwrap();
        assert_eq!(msg["isFlagged"], json!(true));
        assert_eq!(msg["keywords"], json!(["$flagged"]));

        // The authority applies the flag and re-serves the base with it; confirm
        // retires the now-redundant pending op (the base carries the effect).
        handle
            .ingest_batch_json(
                &json!([{
                    "message": {
                        "messageId": "m1",
                        "projection": {
                            "id": "m1", "sourceId": "primary",
                            "receivedAt": "2026-04-29T10:00:00Z",
                            "mailboxIds": ["inbox"], "keywords": ["$flagged"],
                            "isRead": false, "isFlagged": true, "subject": "m1"
                        },
                        "deleted": false, "countDeltas": []
                    }
                }])
                .to_string(),
            )
            .unwrap();
        assert!(!handle.settle("op1", "confirmed").unwrap());
        assert!(!handle.has_pending());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&handle.message_json("m1")).unwrap()
                ["isFlagged"],
            json!(true)
        );
    }
}
