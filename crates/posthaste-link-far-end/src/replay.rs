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

/// Default backlog depth per subscriber, mirroring the runtime session's frame
/// broadcast capacity (`512`): sized so ordinary bursts never overflow, with the
/// collapse fallback as the safety net.
pub const DEFAULT_BACKLOG_CAPACITY: usize = 512;

/// A seam frame stamped with its monotonic per-subscriber sequence — the
/// down-channel's resume cursor (D46). This is the **generic, seam-agnostic**
/// wire envelope owned by the engine crate: the seq rides *alongside* the frame
/// (`{ "seq": N, "frame": { .. } }`), never inside it, so a frame stays named by
/// its emitter (D1/D39) and one frame vocabulary serves each seam (XIV). Both
/// seams reuse this one envelope over their own `Frame` type.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sequenced<Frame> {
    pub seq: u64,
    pub frame: Frame,
}

impl<Frame> Sequenced<Frame> {
    pub fn new(seq: u64, frame: Frame) -> Self {
        Self { seq, frame }
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

struct Backlog<Frame> {
    next_seq: u64,
    /// `(seq, frame)` in emission order, bounded at `capacity`; the oldest is
    /// dropped when full. `oldest_retained_seq` tracks the front so a resume can
    /// tell "already replayed" from "dropped, must collapse".
    buffer: VecDeque<Sequenced<Frame>>,
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

    /// A store with an explicit per-subscriber backlog depth.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            subscribers: Mutex::new(HashMap::new()),
            capacity: capacity.max(1),
        }
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
        let stamped = Sequenced { seq, frame };
        backlog.buffer.push_back(stamped.clone());
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
        if after >= highest {
            // Caller is already current (or ahead): nothing to replay.
            return Resume::Replay(Vec::new());
        }
        let oldest_retained = backlog.buffer.front().map(|s| s.seq);
        match oldest_retained {
            // The frame immediately after `after` is still retained → serviceable.
            Some(oldest) if oldest <= after + 1 => Resume::Replay(
                backlog
                    .buffer
                    .iter()
                    .filter(|s| s.seq > after)
                    .cloned()
                    .collect(),
            ),
            // The resume point was evicted (or the buffer is empty but frames
            // were issued) → the subscriber must collapse.
            _ => Resume::Collapse,
        }
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
        assert_eq!(store.record(&"a", 'x').seq, 1);
        assert_eq!(store.record(&"a", 'y').seq, 2);
        // A distinct subscriber has its own seq domain.
        assert_eq!(store.record(&"b", 'z').seq, 1);
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
                    frames.iter().map(|s| s.frame).collect::<Vec<_>>(),
                    vec!['b', 'c']
                );
                assert_eq!(frames.iter().map(|s| s.seq).collect::<Vec<_>>(), vec![2, 3]);
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
