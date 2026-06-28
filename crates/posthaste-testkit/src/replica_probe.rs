//! A client-replica probe + flicker detector for runtime-layer diagnosis.
//!
//! [`ReplicaProbe`] drives the real [`posthaste_link_replica::EntityStore`] (the
//! same reconciliation/optimism code the browser runs via WASM) from a captured
//! [`RuntimeFrame`] stream, mirroring the essential path of the web
//! `entityStoreAdapter`: a user mutation is `accept_mutation`'d (optimism) before
//! it runs; `message.updated` Notification frames feed `ingest_batch`; a
//! `MutationNotification` verdict drives `settle`. After each frame the probe
//! records a [`RenderSnapshot`] (the projected rows a renderer would bind to),
//! so [`FlickerLog`] can assert no row's observable field reverts and no row
//! disappears-then-reappears during a mutation interleaved with provider sync.
//!
//! This is the Layer-B half of the flicker fixture: feed the *real* frames a
//! mutation + sync emits (Layer A) into the *real* reconciliation engine and
//! catch the flicker deterministically, without a browser or `posthastectl`. The
//! adapter port here is deliberately thin; the flicker-prone logic is the shared
//! `EntityStore`, not this glue.

use posthaste_link_core::{MessageAssertion, MutationId, SettlementOutcome};
use posthaste_link_replica::{
    EntityStore, SortDirection, SortKey, StoreUpdate, ViewPredicate, ViewRow,
};
use posthaste_runtime_contract::{MutationNotification, RuntimeFrame};
use serde_json::Value;

/// One row as a renderer would see it: the projected (base + folded optimism)
/// observable fields, in view order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedRow {
    pub message_id: String,
    pub is_read: bool,
    pub is_flagged: bool,
}

/// The projected rows of a view at one point in the frame stream, tagged with the
/// frame that produced it.
#[derive(Clone, Debug)]
pub struct RenderSnapshot {
    pub after: String,
    pub rows: Vec<RenderedRow>,
}

/// Drives a real `EntityStore` from runtime frames + records the render
/// trajectory of one view.
pub struct ReplicaProbe {
    store: EntityStore,
    view_id: String,
    log: Vec<RenderSnapshot>,
}

impl ReplicaProbe {
    /// Open a mail-list view scoped to `mailbox_id`, seeded from the runtime's
    /// initial `ViewSnapshot` rows (mirrors the adapter's `openMailListView`:
    /// register the view, seed each row's base, place the rows). Records the
    /// initial render.
    pub fn open_view(view_id: &str, mailbox_id: &str, initial: &Value) -> Self {
        let mut store = EntityStore::new();
        store.register_view(
            view_id,
            ViewPredicate::InMailboxes(vec![mailbox_id.to_string()]),
            "receivedAt".to_string(),
            SortDirection::Desc,
            // Watermark `None` = the held window reaches BOTTOM (the view holds
            // every match), so sync-delivered siblings are self-placed. Fine for
            // a small fixture mailbox fully covered by the first page.
            None,
        );
        let rows = mail_list_rows(initial);
        store.ingest_batch(projection_batch_from_rows(rows));
        store.set_view_rows(view_id, view_rows_from(rows), None);
        store.drain_dirty();
        let mut probe = Self {
            store,
            view_id: view_id.to_string(),
            log: Vec::new(),
        };
        probe.record("open");
        probe
    }

    /// Accept an optimistic mutation (mirrors the adapter accepting the op before
    /// it runs against the runtime). Records the post-optimism render.
    pub fn accept_mutation(
        &mut self,
        client_mutation_id: &str,
        message_id: &str,
        assertion: MessageAssertion,
    ) {
        self.store.accept_mutation(
            MutationId(client_mutation_id.to_string()),
            message_id,
            assertion,
        );
        self.record(&format!("accept({client_mutation_id})"));
    }

    /// Apply one captured runtime frame the way the adapter would, recording the
    /// render after frames that can move the view. Frames irrelevant to the
    /// store (other views, deltas the adapter re-derives from `message.updated`)
    /// are ignored.
    pub fn apply_frame(&mut self, frame: &RuntimeFrame) {
        match frame {
            RuntimeFrame::ViewSnapshot {
                view_id, snapshot, ..
            }
            | RuntimeFrame::ViewReplace {
                view_id, snapshot, ..
            } if view_id.as_str() == self.view_id => {
                let data = serde_json::to_value(snapshot).unwrap_or(Value::Null);
                let rows = mail_list_rows(&data);
                self.store.ingest_batch(projection_batch_from_rows(rows));
                self.store
                    .set_view_rows(&self.view_id, view_rows_from(rows), None);
                self.store.drain_dirty();
                self.record("viewReplace");
            }
            RuntimeFrame::Notification { kind, payload, .. } if kind == "message.updated" => {
                if let Some(update) = message_update_from_event(payload) {
                    self.store.ingest_batch(vec![update]);
                    self.store.drain_dirty();
                    self.record("message.updated");
                }
            }
            RuntimeFrame::MutationNotification {
                client_mutation_id,
                notification,
                ..
            } => {
                let outcome = match notification {
                    MutationNotification::Confirmed => SettlementOutcome::Confirmed,
                    MutationNotification::Rejected { .. } => SettlementOutcome::Failed,
                };
                self.store.settle(
                    &MutationId(client_mutation_id.as_str().to_string()),
                    outcome,
                );
                self.store.drain_dirty();
                self.record(&format!("settle({})", client_mutation_id.as_str()));
            }
            _ => {}
        }
    }

    /// The recorded render trajectory.
    pub fn into_log(self) -> FlickerLog {
        FlickerLog {
            snapshots: self.log,
        }
    }

    /// The current projected rows (base + folded optimism), in view order.
    fn render(&self) -> Vec<RenderedRow> {
        let Some(rows) = self.store.view_rows(&self.view_id) else {
            return Vec::new();
        };
        rows.iter()
            .map(|row| {
                let projection = self.store.message(&row.message_id).unwrap_or(Value::Null);
                RenderedRow {
                    message_id: row.message_id.clone(),
                    is_read: projection
                        .get("isRead")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    is_flagged: projection
                        .get("isFlagged")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                }
            })
            .collect()
    }

    fn record(&mut self, after: &str) {
        let rows = self.render();
        self.log.push(RenderSnapshot {
            after: after.to_string(),
            rows,
        });
    }
}

/// The recorded render trajectory of a view across a frame stream, with flicker
/// assertions.
pub struct FlickerLog {
    pub snapshots: Vec<RenderSnapshot>,
}

impl FlickerLog {
    /// The sequence of a message's `is_read` across snapshots where it is
    /// present.
    fn field_sequence(&self, message_id: &str, field: fn(&RenderedRow) -> bool) -> Vec<bool> {
        self.snapshots
            .iter()
            .filter_map(|snap| snap.rows.iter().find(|r| r.message_id == message_id))
            .map(field)
            .collect()
    }

    /// The presence sequence of a message across snapshots (true = present).
    fn presence_sequence(&self, message_id: &str) -> Vec<bool> {
        self.snapshots
            .iter()
            .map(|snap| snap.rows.iter().any(|r| r.message_id == message_id))
            .collect()
    }

    /// Assert no observable flicker for `message_id`: neither `is_read` nor
    /// `is_flagged` reverts (a value reappearing after changing), and the row
    /// never disappears then reappears. Reverts are the visible flicker.
    pub fn assert_no_flicker(&self, message_id: &str) {
        assert!(
            !reverts(&self.presence_sequence(message_id)),
            "row {message_id} disappeared then reappeared (presence flicker)\n{}",
            self.dump()
        );
        assert!(
            !reverts(&self.field_sequence(message_id, |r| r.is_read)),
            "row {message_id} isRead reverted (read flicker)\n{}",
            self.dump()
        );
        assert!(
            !reverts(&self.field_sequence(message_id, |r| r.is_flagged)),
            "row {message_id} isFlagged reverted (flag flicker)\n{}",
            self.dump()
        );
    }

    /// A human-readable trajectory dump for diagnosis.
    pub fn dump(&self) -> String {
        let mut out = String::from("render trajectory:\n");
        for snap in &self.snapshots {
            let rows: Vec<String> = snap
                .rows
                .iter()
                .map(|r| {
                    format!(
                        "{}{}{}",
                        r.message_id,
                        if r.is_read { "·read" } else { "·unread" },
                        if r.is_flagged { "·flagged" } else { "" }
                    )
                })
                .collect();
            out.push_str(&format!("  [{:>16}] {}\n", snap.after, rows.join(", ")));
        }
        out
    }
}

/// Whether a boolean sequence reverts: some value appears, is replaced, and
/// later reappears (`a … b … a`) — the signature of a visible flicker.
fn reverts(seq: &[bool]) -> bool {
    for i in 0..seq.len() {
        let mut left = false;
        for j in (i + 1)..seq.len() {
            if seq[j] != seq[i] {
                left = true;
            } else if left {
                return true;
            }
        }
    }
    false
}

// --- adapter helpers (faithful to entityStoreAdapter.ts) --------------------

/// The `rows` array of a serialized `MailListViewState` snapshot.
fn mail_list_rows(snapshot: &Value) -> &[Value] {
    snapshot
        .get("data")
        .and_then(|d| d.get("rows"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Seed message bases for every served row (`projectionBatchFromRows`).
fn projection_batch_from_rows(rows: &[Value]) -> Vec<StoreUpdate> {
    rows.iter()
        .filter_map(|row| {
            let projection = row.get("projection")?;
            let message_id = projection.get("id")?.as_str()?.to_string();
            Some(StoreUpdate::Message {
                message_id,
                projection: projection.clone(),
                deleted: false,
                count_deltas: Vec::new(),
            })
        })
        .collect()
}

/// Map served rows to the store's `ViewRow`s (`toStoreRow`).
fn view_rows_from(rows: &[Value]) -> Vec<ViewRow> {
    rows.iter()
        .filter_map(|row| {
            let projection = row.get("projection")?;
            let id = projection.get("id")?.as_str()?.to_string();
            let source = projection
                .get("sourceId")
                .and_then(Value::as_str)
                .unwrap_or("");
            let received_at = projection
                .get("receivedAt")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Some(ViewRow {
                row_key: format!("{source}:{id}"),
                message_id: id.clone(),
                sort_key: SortKey {
                    received_at,
                    message_id: id,
                },
            })
        })
        .collect()
}

/// Extract a `StoreUpdate::Message` from a serialized `message.updated`
/// DomainEvent (`ingestMessageEvent`): the event's `payload` holds
/// `{messageId, projection, countDeltas, deleted}`.
fn message_update_from_event(event: &Value) -> Option<StoreUpdate> {
    let inner = event.get("payload")?;
    let message_id = inner.get("messageId")?.as_str()?.to_string();
    let deleted = inner
        .get("deleted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let projection = inner.get("projection").cloned().unwrap_or(Value::Null);
    if !deleted && projection.is_null() {
        return None;
    }
    Some(StoreUpdate::Message {
        message_id,
        projection,
        deleted,
        count_deltas: Vec::new(),
    })
}
