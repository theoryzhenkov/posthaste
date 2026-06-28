use super::*;

/// One reversible-operation step in the per-account `rev_log` — the Phase 2
/// server-authoritative undo/redo log. The `diff` is a `MessageChangeDiff`
/// JSON, opaque at this layer: the semantics live in `posthaste-link-core` /
/// the client. The store holds it as JSON text; the `RevLog` synced view mirrors
/// it to every device.
///
/// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevLogStep {
    /// Globally-orderable id (ULID); the cursor key.
    pub step_id: String,
    /// Per-account monotonic append order (the sync delta cursor).
    pub seq: u32,
    pub message_id: String,
    pub source_id: String,
    /// `MessageChangeDiff` JSON (`{keywords, mailboxes}{added, removed}`).
    pub diff: Value,
    pub created_at: String,
}

/// The per-account undo/redo cursor. `cursor_step_id` = the topmost APPLIED
/// step (`None` = all undone, cursor at -1); `redo_tail` = the undone step_ids
/// above the cursor, in `seq` order. The default is the fresh-account state
/// (no cursor, no redo tail).
///
/// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevCursor {
    pub cursor_step_id: Option<String>,
    pub redo_tail: Vec<String>,
}

/// A snapshot of an account's `rev_log` + cursor — the read result behind the
/// `RevLog` synced view. The view serves this on open / reconnect; later slices
/// broadcast append + cursor deltas over the same view subscription.
///
/// @spec docs/eph/DESIGN-L2-undo-redo-revlog-contract
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevLogSnapshot {
    pub steps: Vec<RevLogStep>,
    pub cursor: RevCursor,
}
