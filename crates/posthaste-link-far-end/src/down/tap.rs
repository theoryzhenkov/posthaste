//! The **tap** (RFC-L2-scripting D52): a read-only far-end for the fact-carrying
//! channel kind (§3).
//!
//! A tap is the down-channel half instantiated *alone* — no dedup, no settlement
//! sinks (a read-only consumer has no writes to dedup and no settlements to
//! route; §5, sinkless). It differs from a plain [`super::replay::ReplayStore`]
//! seam in two ways that the fact-carrying kind demands (§3):
//!
//! 1. **Durable replay.** The in-memory backlog is replaced by a durable
//!    [`FactLog`]: for facts, history is the payload, so replay must survive a
//!    process restart and a bounded-buffer overflow.
//! 2. **The gap frame, not collapse.** A resume whose cursor fell before the
//!    log's truncation point cannot be served from durable history. Instead of
//!    collapsing to current state (data loss for facts), the tap emits the
//!    explicit **gap frame** (the `Reset` element reinterpreted, [`Sequenced::
//!    gap`]) — never silent, never `Collapse` (fixes N8).
//!
//! Server-side per-consumer state is exactly one **reaper-managed registry
//! entry** (§5.4): the subscriber's opaque seq cursor + last-seen tick, evicted
//! by the same tick discipline as the D48 dedup/sink reapers (a shared `now`
//! tick the mount drives; never ambient time). This is machinery only — S2
//! mounts it on `/v1/events`; the runtime binds the [`FactLog`] over its
//! `event_log`.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

use crate::down::fact_log::{FactLog, FactLogError};
use crate::down::replay::Sequenced;

/// Default idle TTL for a tap subscriber's registry entry, in the mount's `now`
/// tick units (**seconds** as the runtime/authority drive it) — `300` = five
/// minutes of no resume/keepalive. A stateless consumer holds only a seq cursor
/// (§5.1), so a dropped consumer leaves just this entry; the reaper reclaims it
/// after the TTL. Sized to survive an ordinary reconnect gap while bounding
/// registry growth under consumer churn. Flagged for review.
pub const DEFAULT_TAP_TTL: u64 = 300;

/// The outcome of [`Tap::subscribe`] — where the subscriber resumes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TapResume<Fact> {
    /// A fresh subscription (`after_seq` was `None`): nothing to replay from
    /// history. The consumer attaches at the live head (§5.3: read state via the
    /// Api as-of a seq, then tail the tap from that seq).
    Fresh,
    /// Serviceable from durable history: replay these facts (every retained fact
    /// with `seq > after_seq`, filter-narrowed), then continue live.
    Replay(Vec<Sequenced<Fact>>),
    /// The cursor fell before the log's truncation point: an explicit **gap**.
    /// The consumer receives the gap frame ([`Sequenced::gap`]) carrying
    /// `highest_seq` and decides how to recover — never a silent drop, never a
    /// collapse (§3, N8).
    Gap { highest_seq: u64 },
}

impl<Fact> TapResume<Fact> {
    /// The wire elements this resume delivers, in order: the replayed facts, or
    /// the single gap frame (the `Reset` element reinterpreted, §3), or nothing
    /// for a fresh attach. This is where "collapse" becomes "gap" on the wire.
    pub fn into_frames(self) -> Vec<Sequenced<Fact>> {
        match self {
            Self::Fresh => Vec::new(),
            Self::Replay(frames) => frames,
            Self::Gap { highest_seq } => vec![Sequenced::gap(highest_seq)],
        }
    }

    /// Whether this resume opened a gap (for tests/observability).
    pub fn is_gap(&self) -> bool {
        matches!(self, Self::Gap { .. })
    }
}

/// One subscriber's server-side state (§5.4): its opaque seq cursor + the last
/// `now` tick it was seen (resume or keepalive). Nothing else — a stateless
/// consumer carries the rest.
struct Subscriber {
    /// The last seq the subscriber has demonstrably reached (its resume cursor,
    /// advanced by keepalives). Also the ack signal a mount can forward.
    cursor: u64,
    /// The last tick a subscribe/keepalive was observed — drives TTL eviction.
    last_seen: u64,
}

/// A fact-carrying tap: the down half over a durable [`FactLog`], with a
/// reaper-managed subscriber registry and no up-half (D52).
pub struct Tap<L: FactLog, SubscriberId> {
    log: Arc<L>,
    subscribers: Mutex<HashMap<SubscriberId, Subscriber>>,
    ttl: u64,
}

impl<L, SubscriberId> Tap<L, SubscriberId>
where
    L: FactLog,
    SubscriberId: Clone + Eq + Hash,
{
    /// A tap over `log` with the default subscriber TTL ([`DEFAULT_TAP_TTL`]).
    pub fn new(log: Arc<L>) -> Self {
        Self::with_ttl(log, DEFAULT_TAP_TTL)
    }

    /// A tap over `log` with an explicit subscriber-idle TTL (in the mount's
    /// `now` tick units).
    pub fn with_ttl(log: Arc<L>, ttl: u64) -> Self {
        Self {
            log,
            subscribers: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SubscriberId, Subscriber>> {
        self.subscribers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Resolve a (re)subscribe for `id` against the durable log and register /
    /// refresh the subscriber's registry entry at `now`.
    ///
    /// - `after_seq = None` → [`TapResume::Fresh`]; the entry is seeded at the
    ///   live head (`highest_seq`), the attach point a snapshot read tails from.
    /// - `after_seq = Some(after)` past the truncation point → [`TapResume::Gap`]
    ///   (the cursor is seeded at the live head so the consumer re-attaches).
    /// - otherwise → [`TapResume::Replay`] of the durable facts after the cursor
    ///   (filter-narrowed), the entry advanced to `after`.
    ///
    /// The lock is never held across a `FactLog` `await`.
    pub async fn subscribe(
        &self,
        id: &SubscriberId,
        after_seq: Option<u64>,
        filter: Option<L::Filter>,
        now: u64,
    ) -> Result<TapResume<L::Fact>, FactLogError> {
        let Some(after) = after_seq else {
            // Fresh: attach at the live head. The consumer reads state via the
            // Api as-of this seq, then tails from here (§5.3).
            let head = self.log.highest_seq().await?;
            self.register(id, head, now);
            return Ok(TapResume::Fresh);
        };

        // The gap test (§3): a cursor before the oldest retained fact cannot be
        // served from durable history. `after + 1` is the first fact the caller
        // wants; if the log no longer retains it, the gap frame stands in.
        let truncation = self.log.truncation_point().await?;
        if truncation > 0 && after + 1 < truncation {
            let head = self.log.highest_seq().await?;
            self.register(id, head, now);
            return Ok(TapResume::Gap { highest_seq: head });
        }

        let frames = self.log.replay(after, filter).await?;
        // Advance the cursor to the furthest fact actually replayed (or the
        // caller's `after` when the tail is empty — it is already current).
        let cursor = frames.last().map(|f| f.seq()).unwrap_or(after);
        self.register(id, cursor, now);
        Ok(TapResume::Replay(frames))
    }

    /// Keepalive / ack for a live subscriber: advance its cursor (monotonically)
    /// and refresh its last-seen tick so the reaper spares it. A live tail that
    /// forwards facts calls this as it delivers them (resume(after_seq) IS the
    /// ack, §5.2); the mount also calls it on any heartbeat. A no-op for an
    /// unknown id (already reaped or never subscribed).
    pub fn touch(&self, id: &SubscriberId, cursor: u64, now: u64) {
        if let Some(sub) = self.lock().get_mut(id) {
            sub.cursor = sub.cursor.max(cursor);
            sub.last_seen = now;
        }
    }

    /// Register or refresh a subscriber entry (cursor advances monotonically).
    fn register(&self, id: &SubscriberId, cursor: u64, now: u64) {
        let mut subs = self.lock();
        let entry = subs.entry(id.clone()).or_insert(Subscriber {
            cursor,
            last_seen: now,
        });
        entry.cursor = entry.cursor.max(cursor);
        entry.last_seen = now;
    }

    /// Evict every subscriber whose registry entry has been idle (no resume /
    /// keepalive) for at least the TTL, driven by the explicit `now` tick — the
    /// same tick discipline as the D48 dedup/sink reapers. Returns the reaped
    /// ids so the mount can drop their per-subscriber transport state.
    pub fn reap(&self, now: u64) -> Vec<SubscriberId> {
        let ttl = self.ttl;
        let mut subs = self.lock();
        let reaped: Vec<SubscriberId> = subs
            .iter()
            .filter(|(_, sub)| now.saturating_sub(sub.last_seen) >= ttl)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &reaped {
            subs.remove(id);
        }
        reaped
    }

    /// The subscriber's current cursor, or `None` when unknown.
    pub fn cursor(&self, id: &SubscriberId) -> Option<u64> {
        self.lock().get(id).map(|sub| sub.cursor)
    }

    /// Drop a subscriber's registry entry — the mount's teardown on an explicit
    /// unsubscribe (mirrors the DELETE path the M28 reaper backstops).
    pub fn unsubscribe(&self, id: &SubscriberId) {
        self.lock().remove(id);
    }

    /// The number of live subscribers (for tests/observability).
    pub fn subscriber_count(&self) -> usize {
        self.lock().len()
    }

    /// The live head cursor a fresh consumer attaches at (§5.3).
    pub async fn highest_seq(&self) -> Result<u64, FactLogError> {
        self.log.highest_seq().await
    }

    /// Append a fact through the tap's log — the authoring component's write
    /// path (a read-only binding returns [`FactLogError::ReadOnly`]).
    pub async fn append(&self, fact: L::Fact) -> Result<u64, FactLogError> {
        self.log.append(fact).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex as StdMutex;

    /// An in-memory `FactLog` with a settable truncation point, for the tap's
    /// resume/gap logic. `truncate_below(n)` drops facts with `seq < n` and
    /// reports `n` as the truncation point (the oldest retained seq).
    struct MemLog {
        facts: StdMutex<Vec<Sequenced<char>>>,
        truncation: StdMutex<u64>,
        read_only: bool,
    }

    impl MemLog {
        fn seeded(chars: &[char]) -> Self {
            let facts = chars
                .iter()
                .enumerate()
                .map(|(i, c)| Sequenced::new(i as u64 + 1, *c))
                .collect();
            Self {
                facts: StdMutex::new(facts),
                truncation: StdMutex::new(if chars.is_empty() { 0 } else { 1 }),
                read_only: false,
            }
        }

        fn truncate_below(&self, n: u64) {
            self.facts.lock().unwrap().retain(|f| f.seq() >= n);
            *self.truncation.lock().unwrap() = n;
        }
    }

    #[async_trait]
    impl FactLog for MemLog {
        type Fact = char;
        type Filter = ();

        async fn append(&self, fact: char) -> Result<u64, FactLogError> {
            if self.read_only {
                return Err(FactLogError::ReadOnly);
            }
            let mut facts = self.facts.lock().unwrap();
            let seq = facts.last().map(|f| f.seq()).unwrap_or(0) + 1;
            facts.push(Sequenced::new(seq, fact));
            Ok(seq)
        }

        async fn replay(
            &self,
            after_seq: u64,
            _filter: Option<()>,
        ) -> Result<Vec<Sequenced<char>>, FactLogError> {
            Ok(self
                .facts
                .lock()
                .unwrap()
                .iter()
                .filter(|f| f.seq() > after_seq)
                .cloned()
                .collect())
        }

        async fn highest_seq(&self) -> Result<u64, FactLogError> {
            Ok(self
                .facts
                .lock()
                .unwrap()
                .last()
                .map(|f| f.seq())
                .unwrap_or(0))
        }

        async fn truncation_point(&self) -> Result<u64, FactLogError> {
            Ok(*self.truncation.lock().unwrap())
        }
    }

    fn tap(chars: &[char]) -> Tap<MemLog, &'static str> {
        Tap::with_ttl(Arc::new(MemLog::seeded(chars)), 10)
    }

    #[tokio::test]
    async fn fresh_subscription_attaches_at_the_live_head() {
        let tap = tap(&['a', 'b', 'c']);
        let resume = tap.subscribe(&"s", None, None, 0).await.unwrap();
        assert_eq!(resume, TapResume::Fresh);
        // The entry is seeded at the head so a snapshot read tails from there.
        assert_eq!(tap.cursor(&"s"), Some(3));
    }

    #[tokio::test]
    async fn resume_replays_durable_facts_after_the_cursor() {
        let tap = tap(&['a', 'b', 'c']);
        match tap.subscribe(&"s", Some(1), None, 0).await.unwrap() {
            TapResume::Replay(frames) => {
                assert_eq!(
                    frames
                        .iter()
                        .map(|f| *f.frame().unwrap())
                        .collect::<Vec<_>>(),
                    vec!['b', 'c']
                );
            }
            other => panic!("expected replay, got {other:?}"),
        }
        assert_eq!(tap.cursor(&"s"), Some(3), "cursor advanced to the tail");
    }

    #[tokio::test]
    async fn resume_at_head_replays_nothing_but_stays_current() {
        let tap = tap(&['a', 'b']);
        match tap.subscribe(&"s", Some(2), None, 0).await.unwrap() {
            TapResume::Replay(frames) => assert!(frames.is_empty()),
            other => panic!("expected empty replay, got {other:?}"),
        }
        assert_eq!(tap.cursor(&"s"), Some(2));
    }

    // §3: a resume from before the truncation point yields the gap frame — never
    // a silent drop, never a collapse. The gap frame IS the `Reset` element
    // reinterpreted, carrying the live head as the re-attach cursor.
    #[tokio::test]
    async fn resume_past_the_truncation_point_yields_the_gap_frame() {
        let log = Arc::new(MemLog::seeded(&['a', 'b', 'c', 'd', 'e']));
        log.truncate_below(4); // seqs 1..=3 dropped; oldest retained = 4
        let tap = Tap::<MemLog, &'static str>::with_ttl(log, 10);

        // A cursor at seq 1 wants seq 2 next, which is gone → gap.
        let resume = tap.subscribe(&"s", Some(1), None, 0).await.unwrap();
        assert!(resume.is_gap(), "cursor before truncation opens a gap");
        assert_eq!(resume, TapResume::Gap { highest_seq: 5 });

        // The gap frame on the wire is the reinterpreted Reset carrying the head.
        let frames = resume.into_frames();
        assert_eq!(frames, vec![Sequenced::gap(5)]);
        assert!(
            frames[0].is_reset(),
            "gap rides the Reset wire element (§3)"
        );
    }

    // The boundary: a cursor exactly at the truncation point is still
    // serviceable (its next fact is the oldest retained one), not a gap.
    #[tokio::test]
    async fn resume_at_the_truncation_boundary_replays_not_gaps() {
        let log = Arc::new(MemLog::seeded(&['a', 'b', 'c', 'd']));
        log.truncate_below(3); // oldest retained = 3
        let tap = Tap::<MemLog, &'static str>::with_ttl(log, 10);
        // Cursor at 2 wants 3, which is retained → replay, no gap.
        match tap.subscribe(&"s", Some(2), None, 0).await.unwrap() {
            TapResume::Replay(frames) => {
                assert_eq!(
                    frames.iter().map(|f| f.seq()).collect::<Vec<_>>(),
                    vec![3, 4]
                );
            }
            other => panic!("boundary must replay, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn idle_subscriber_is_reaped_after_the_ttl() {
        let tap = tap(&['a']);
        tap.subscribe(&"s", None, None, 0).await.unwrap();
        assert_eq!(tap.subscriber_count(), 1);
        assert!(tap.reap(5).is_empty(), "within ttl (5 - 0 < 10)");
        // A keepalive refreshes the last-seen tick, sparing it.
        tap.touch(&"s", 0, 8);
        assert!(tap.reap(15).is_empty(), "keepalive spared it (15 - 8 < 10)");
        assert_eq!(tap.reap(20), vec!["s"], "20 - 8 >= 10 → reaped");
        assert_eq!(tap.subscriber_count(), 0);
    }

    #[tokio::test]
    async fn append_is_read_only_for_a_read_only_binding() {
        let log = Arc::new(MemLog {
            facts: StdMutex::new(Vec::new()),
            truncation: StdMutex::new(0),
            read_only: true,
        });
        let tap = Tap::<MemLog, &'static str>::new(log);
        assert!(matches!(tap.append('x').await, Err(FactLogError::ReadOnly)));
    }
}
