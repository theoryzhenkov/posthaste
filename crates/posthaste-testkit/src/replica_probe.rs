//! Flicker detectors for runtime-layer diagnosis — the test-only types a
//! render-trajectory assertion needs.
//!
//! `ReplicaProbe` (the Layer-B Rust port of the web `entityStoreAdapter` that
//! drove a real `EntityStore` from captured frames) is **retired**: it was
//! duplicated, divergent glue. The flicker-prone logic is the shared
//! `EntityStore`, and the REAL adapter — Layer D (`apps/web/test/renderProbe.tsx`),
//! real WASM Layer C (`apps/web/test/replicaAbsorptionRetire.test.ts`), and the
//! Playwright e2e — covers it faithfully without a hand-ported copy. A future
//! Rust probe can drive the real `EntityStore` directly and feed these detectors
//! without re-introducing the adapter port.
//!
//! What stays is the detector layer: [`RenderSnapshot`] / [`RenderedRow`] (one
//! projected row-set tagged with the frame that produced it) and [`FlickerLog`]
//! (the trajectory + `assert_no_flicker`: no observable field reverts, no row
//! disappears-then-reappears).

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

/// The recorded render trajectory of a view across a frame stream, with flicker
/// assertions. Construct directly from a `Vec<RenderSnapshot>` (the fields are
/// public) — a probe records one snapshot per frame, then asserts.
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
