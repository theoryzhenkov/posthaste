//! The seq-backlog replay sub-store (RFC D46).
//!
//! One replay mechanism, mounted at both seams: a **monotonic per-subscriber
//! seq**, a **bounded backlog buffer** of recently emitted `(seq, frame)`, and
//! **resume from `after_seq`** on reconnect. This is the client seam's existing
//! semantics generalized — coverage says WHAT to stream, seq says WHERE to
//! resume.
//!
//! When a subscriber resumes from an `after_seq` older than the retained
//! backlog (the buffer overflowed and dropped it), or the far-end's live channel
//! reports a broadcast `Lagged`, the store surfaces a **collapse-to-current-state**
//! signal ([`Resume::Collapse`]). The store only *names* the need; the
//! assembling far-end supplies what collapse means for its frames (re-serve open
//! views + the mutation window for the runtime seam; re-assert the base / signal
//! resync for the authority server seam).

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Default backlog depth per subscriber, mirroring the runtime link's frame
/// broadcast capacity (`512`): sized so ordinary bursts never overflow, with the
/// collapse fallback as the safety net.
pub const DEFAULT_BACKLOG_CAPACITY: usize = 512;

/// A down-channel wire element (D46/D49). This is the **generic, seam-agnostic**
/// wire envelope owned by the engine crate: the seq rides *alongside* the frame,
/// never inside it, so a frame stays named by its emitter (D1/D39) and one frame
/// vocabulary serves each seam (XIV). Both seams reuse this one envelope over
/// their own `Frame` type.
///
/// Two shapes ride the wire (serde-internally-tagged on `kind`, camelCase — the
/// envelope is young enough that this atomic shape change is fine per §9):
///
/// - `{ "kind": "frame", "seq": N, "frame": { .. } }` — a stamped down-frame
///   carrying its monotonic per-subscriber seq.
/// - `{ "kind": "reset", "highestSeq": N }` — a **control** element (D49): the
///   subscriber's resume point fell out of the backlog (or claimed a seq never
///   issued), so it must collapse to current state and re-seed. `highest_seq` is
///   the far-end's current cursor the subscriber adopts, so it stops gap-detecting
///   against the lost seqs. Emitted as the first stream element on a Collapse.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Sequenced<Frame> {
    /// A stamped down-frame carrying its monotonic per-subscriber seq.
    Frame { seq: u64, frame: Frame },
    /// A reset control element — the subscriber must collapse-and-reseed and
    /// adopt `highest_seq` as its new resume cursor (D49).
    Reset { highest_seq: u64 },
}

impl<Frame> Sequenced<Frame> {
    /// A stamped data element.
    pub fn new(seq: u64, frame: Frame) -> Self {
        Self::Frame { seq, frame }
    }

    /// A reset control element (D49).
    pub fn reset(highest_seq: u64) -> Self {
        Self::Reset { highest_seq }
    }

    /// The resume cursor this element advances the subscriber to: a frame's seq,
    /// or a reset's `highest_seq` — both name "the seq the subscriber is now at".
    pub fn seq(&self) -> u64 {
        match self {
            Self::Frame { seq, .. } => *seq,
            Self::Reset { highest_seq } => *highest_seq,
        }
    }

    /// The carried frame, or `None` for a reset control element.
    pub fn frame(&self) -> Option<&Frame> {
        match self {
            Self::Frame { frame, .. } => Some(frame),
            Self::Reset { .. } => None,
        }
    }

    /// Whether this is a reset control element.
    pub fn is_reset(&self) -> bool {
        matches!(self, Self::Reset { .. })
    }
}

/// The outcome of [`ReplayStore::resume`].
pub enum Resume<Frame> {
    /// A fresh subscription (`after_seq` was `None`): nothing to replay; start
    /// the live stream.
    Fresh,
    /// Resume is serviceable from the backlog: replay these buffered frames
    /// (every retained frame with `seq > after_seq`), then continue live.
    Replay(Vec<Sequenced<Frame>>),
    /// `after_seq` is older than the retained backlog (the buffer dropped it):
    /// the subscriber must collapse to current state. The far-end supplies what
    /// that means for its frames.
    Collapse,
}

/// A stamped frame retained in the backlog. Only the `Frame` shape is ever
/// retained (a `Reset` is a transient control signal, never buffered), so the
/// backlog stores this private struct and lifts it to [`Sequenced::Frame`] on
/// the way out.
struct Stamped<Frame> {
    seq: u64,
    frame: Frame,
}

struct Backlog<Frame> {
    next_seq: u64,
    /// Stamped frames in emission order, bounded at `capacity`; the oldest is
    /// dropped when full. The front seq tells "already replayed" from "dropped,
    /// must collapse". At `capacity == 0` (collapse-always, D50) nothing is
    /// retained, so every resume from a stale cursor collapses.
    buffer: VecDeque<Stamped<Frame>>,
}

impl<Frame> Backlog<Frame> {
    fn new() -> Self {
        Self {
            next_seq: 1,
            buffer: VecDeque::new(),
        }
    }
}

/// The shared seq-backlog replay store, keyed per subscriber `LinkId`.
pub struct ReplayStore<LinkId, Frame> {
    subscribers: Mutex<HashMap<LinkId, Backlog<Frame>>>,
    capacity: usize,
}

impl<LinkId, Frame> ReplayStore<LinkId, Frame>
where
    LinkId: Clone + Eq + Hash,
    Frame: Clone,
{
    /// A store with the default backlog depth ([`DEFAULT_BACKLOG_CAPACITY`]).
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_BACKLOG_CAPACITY)
    }

    /// A store with an explicit per-subscriber backlog depth. `0` is the
    /// **collapse-always** mode (D50): the seq counter still advances (so a
    /// resume can compare cursors and a `Reset` carries the current cursor), but
    /// nothing is retained — every resume from a stale cursor collapses. The
    /// runtime far-end mounts this: its collapse re-serves whole snapshots, so a
    /// per-frame replay backlog buys nothing.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            subscribers: Mutex::new(HashMap::new()),
            capacity,
        }
    }

    /// The collapse-always store (D50): capacity 0 — every resume from a stale
    /// cursor collapses to current state rather than replaying a backlog.
    pub fn collapse_always() -> Self {
        Self::with_capacity(0)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<LinkId, Backlog<Frame>>> {
        self.subscribers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Assign the next monotonic seq to a frame for `link`, retain it in the
    /// bounded backlog (dropping the oldest on overflow), and return the stamped
    /// frame the far-end emits on the wire.
    pub fn record(&self, link: &LinkId, frame: Frame) -> Sequenced<Frame> {
        let mut subs = self.lock();
        let backlog = subs.entry(link.clone()).or_insert_with(Backlog::new);
        let seq = backlog.next_seq;
        backlog.next_seq += 1;
        let stamped = Sequenced::Frame {
            seq,
            frame: frame.clone(),
        };
        backlog.buffer.push_back(Stamped { seq, frame });
        while backlog.buffer.len() > self.capacity {
            backlog.buffer.pop_front();
        }
        stamped
    }

    /// Resolve a subscribe/resubscribe: `after_seq = None` is a fresh stream;
    /// otherwise replay the retained frames after it, or signal collapse when the
    /// resume point has been dropped from the backlog.
    pub fn resume(&self, link: &LinkId, after_seq: Option<u64>) -> Resume<Frame> {
        let Some(after) = after_seq else {
            return Resume::Fresh;
        };
        let subs = self.lock();
        let Some(backlog) = subs.get(link) else {
            // No history for this subscriber yet. If it claims a prior seq we
            // never issued, we cannot serve it — collapse to be safe.
            return if after == 0 {
                Resume::Fresh
            } else {
                Resume::Collapse
            };
        };
        let highest = backlog.next_seq.saturating_sub(1);
        if after == highest {
            // Caller is exactly current: nothing to replay.
            return Resume::Replay(Vec::new());
        }
        if after > highest {
            // Caller claims a seq we never issued (a stale/forged cursor across a
            // far-end restart, or a bug) — we cannot serve it. Collapse ([5];
            // was a silent empty replay).
            return Resume::Collapse;
        }
        let oldest_retained = backlog.buffer.front().map(|s| s.seq);
        match oldest_retained {
            // The frame immediately after `after` is still retained → serviceable.
            Some(oldest) if oldest <= after + 1 => Resume::Replay(
                backlog
                    .buffer
                    .iter()
                    .filter(|s| s.seq > after)
                    .map(|s| Sequenced::Frame {
                        seq: s.seq,
                        frame: s.frame.clone(),
                    })
                    .collect(),
            ),
            // The resume point was evicted (or the buffer is empty but frames
            // were issued — the collapse-always mode, D50) → the subscriber must
            // collapse.
            _ => Resume::Collapse,
        }
    }

    /// Advance a subscriber's monotonic per-link seq counter and return the new
    /// seq **without buffering a frame**. For a collapse-always seam (D50) whose
    /// frames carry their own seq field on the wire (the runtime link stream):
    /// the shared store owns the counter + collapse detection, but retains no
    /// backlog, so the caller stamps the returned seq into its own frame type.
    pub fn stamp(&self, link: &LinkId) -> u64 {
        let mut subs = self.lock();
        let backlog = subs.entry(link.clone()).or_insert_with(Backlog::new);
        let seq = backlog.next_seq;
        backlog.next_seq += 1;
        seq
    }

    /// The highest seq issued for a subscriber, or `0` if none — the resume
    /// cursor a far-end hands back after a live collapse.
    pub fn highest_seq(&self, link: &LinkId) -> u64 {
        self.lock()
            .get(link)
            .map(|b| b.next_seq.saturating_sub(1))
            .unwrap_or(0)
    }

    /// Drop a subscriber's backlog — teardown when its link closes.
    pub fn purge(&self, link: &LinkId) {
        self.lock().remove(link);
    }
}

impl<LinkId, Frame> Default for ReplayStore<LinkId, Frame>
where
    LinkId: Clone + Eq + Hash,
    Frame: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_assigns_monotonic_per_subscriber_seq() {
        let store: ReplayStore<&str, char> = ReplayStore::new();
        assert_eq!(store.record(&"a", 'x').seq(), 1);
        assert_eq!(store.record(&"a", 'y').seq(), 2);
        // A distinct subscriber has its own seq domain.
        assert_eq!(store.record(&"b", 'z').seq(), 1);
    }

    #[test]
    fn reset_element_carries_the_highest_seq() {
        let reset: Sequenced<char> = Sequenced::reset(7);
        assert!(reset.is_reset());
        assert_eq!(reset.seq(), 7);
        assert!(reset.frame().is_none());
        // Round-trips through the internally-tagged wire shape.
        let json = serde_json::to_string(&reset).unwrap();
        assert_eq!(json, r#"{"kind":"reset","highestSeq":7}"#);
        assert_eq!(serde_json::from_str::<Sequenced<char>>(&json).unwrap(), reset);
    }

    #[test]
    fn fresh_subscription_has_nothing_to_replay() {
        let store: ReplayStore<&str, char> = ReplayStore::new();
        store.record(&"a", 'x');
        assert!(matches!(store.resume(&"a", None), Resume::Fresh));
    }

    #[test]
    fn resume_replays_frames_after_the_cursor() {
        let store: ReplayStore<&str, char> = ReplayStore::new();
        for f in ['a', 'b', 'c'] {
            store.record(&"s", f);
        }
        match store.resume(&"s", Some(1)) {
            Resume::Replay(frames) => {
                assert_eq!(
                    frames.iter().map(|s| *s.frame().unwrap()).collect::<Vec<_>>(),
                    vec!['b', 'c']
                );
                assert_eq!(frames.iter().map(|s| s.seq()).collect::<Vec<_>>(), vec![2, 3]);
            }
            _ => panic!("resume within the backlog must replay"),
        }
    }

    #[test]
    fn resume_at_head_replays_nothing() {
        let store: ReplayStore<&str, char> = ReplayStore::new();
        store.record(&"s", 'a');
        store.record(&"s", 'b');
        match store.resume(&"s", Some(2)) {
            Resume::Replay(frames) => assert!(frames.is_empty()),
            _ => panic!("a caller already at head replays nothing"),
        }
    }

    #[test]
    fn overflowed_backlog_signals_collapse() {
        let store: ReplayStore<&str, u32> = ReplayStore::with_capacity(2);
        for i in 0..5u32 {
            store.record(&"s", i); // seqs 1..=5; only seqs 4,5 retained
        }
        // Resuming from seq 1 (long dropped) must collapse.
        assert!(matches!(store.resume(&"s", Some(1)), Resume::Collapse));
        // Resuming from a still-retained seq replays.
        assert!(matches!(store.resume(&"s", Some(4)), Resume::Replay(_)));
    }

    #[test]
    fn resume_past_the_highest_seq_collapses() {
        // [5]: a cursor ahead of everything we ever issued cannot be served —
        // collapse rather than returning a misleading empty replay.
        let store: ReplayStore<&str, u32> = ReplayStore::new();
        store.record(&"s", 1);
        store.record(&"s", 2); // highest = 2
        assert!(matches!(store.resume(&"s", Some(2)), Resume::Replay(_)), "at head");
        assert!(matches!(store.resume(&"s", Some(3)), Resume::Collapse), "past head");
    }

    #[test]
    fn collapse_always_mode_never_replays_a_stale_cursor() {
        // D50: capacity 0 retains nothing; the seq counter still advances, so a
        // caller exactly at head replays nothing but any stale cursor collapses.
        let store: ReplayStore<&str, u32> = ReplayStore::collapse_always();
        store.record(&"s", 1);
        store.record(&"s", 2);
        assert_eq!(store.highest_seq(&"s"), 2);
        assert!(matches!(store.resume(&"s", Some(2)), Resume::Replay(_)), "at head");
        assert!(matches!(store.resume(&"s", Some(1)), Resume::Collapse), "stale");
        assert!(matches!(store.resume(&"s", None), Resume::Fresh));
    }

    #[test]
    fn resume_from_unknown_subscriber_with_prior_seq_collapses() {
        let store: ReplayStore<&str, u32> = ReplayStore::new();
        assert!(matches!(store.resume(&"never", Some(9)), Resume::Collapse));
        assert!(matches!(store.resume(&"never", None), Resume::Fresh));
    }

    #[test]
    fn highest_seq_tracks_the_cursor() {
        let store: ReplayStore<&str, char> = ReplayStore::new();
        assert_eq!(store.highest_seq(&"s"), 0);
        store.record(&"s", 'a');
        store.record(&"s", 'b');
        assert_eq!(store.highest_seq(&"s"), 2);
    }
}
