//! The settlement-sink sub-store (RFC D40/D45).
//!
//! Per-`LinkId` settlement routing: a settled mutation's confirmation is routed
//! onto the **originating** subscriber's down-stream only
//! (`settlement-routed-to-origin-runtime`) — never broadcast. Each sink is an
//! unbounded channel (a settlement emitted before the subscriber connects, or
//! while its stream is briefly behind, must never be dropped — a lost settlement
//! strands the near node's outbox entry). A subscriber takes the receiver via
//! [`subscribe`](SettlementSinkStore::subscribe); a reconnect recreates the
//! channel so future settlements drain to the fresh receiver.
//!
//! The **expiry/reaper** story neither hand-rolled far-end had: a sink whose
//! subscriber has been gone for longer than the TTL is reaped, so a churn of
//! transient subscribers cannot leak sinks forever. The reaper is driven by an
//! explicit `now` tick passed by the caller (never ambient time), so it is
//! deterministically testable; the caller decides the tick unit and drives
//! [`reap`](SettlementSinkStore::reap) on its own cadence.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Mutex;

use tokio::sync::mpsc;

/// Default sink TTL, in caller-supplied `now` tick units. A sink whose
/// subscriber has been disconnected for more than this many ticks is reaped.
///
/// The authority server drives the reaper on a wall-clock timer with `now` in
/// **seconds**, so this is `300` = five minutes of subscriber absence — long
/// enough to survive a near node's jittered reconnect window (M9b), short enough
/// to bound sink memory under subscriber churn. Flagged for review.
pub const DEFAULT_SINK_TTL: u64 = 300;

struct Sink<Frame> {
    tx: mpsc::UnboundedSender<Frame>,
    /// `Some` until a subscriber takes it; `None` once handed out. A `None`
    /// receiver with a closed `tx` means the subscriber connected then vanished.
    rx: Option<mpsc::UnboundedReceiver<Frame>>,
    /// The first `now` tick at which this sink was observed with no live
    /// receiver; cleared whenever a subscriber is present. Drives TTL expiry.
    dead_since: Option<u64>,
}

impl<Frame> Sink<Frame> {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            tx,
            rx: Some(rx),
            dead_since: None,
        }
    }

    /// No live subscriber: the receiver was handed out (`rx` is `None`) and then
    /// dropped (`tx` observes all receivers gone). A never-subscribed sink keeps
    /// its buffered receiver, so it is not "dead" (its settlements are waiting).
    fn subscriber_gone(&self) -> bool {
        self.rx.is_none() && self.tx.is_closed()
    }
}

/// The shared per-`LinkId` settlement-sink store.
pub struct SettlementSinkStore<LinkId, Frame> {
    sinks: Mutex<HashMap<LinkId, Sink<Frame>>>,
    ttl: u64,
}

impl<LinkId, Frame> SettlementSinkStore<LinkId, Frame>
where
    LinkId: Clone + Eq + Hash,
{
    /// A store with the default TTL ([`DEFAULT_SINK_TTL`]).
    pub fn new() -> Self {
        Self::with_ttl(DEFAULT_SINK_TTL)
    }

    /// A store with an explicit reaper TTL (in caller `now` tick units).
    pub fn with_ttl(ttl: u64) -> Self {
        Self {
            sinks: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<LinkId, Sink<Frame>>> {
        self.sinks.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Route `frame` onto `link`'s sink, creating it lazily. The send only fails
    /// when the subscriber's receiver has been dropped without a reconnect, in
    /// which case the frame is discarded (safe — the near node reconciles via
    /// the base stream). Liveness/expiry is driven by [`subscribe`](Self::subscribe)
    /// and [`reap`](Self::reap), not by emission.
    pub fn emit(&self, link: &LinkId, frame: Frame) {
        let mut sinks = self.lock();
        let sink = sinks.entry(link.clone()).or_insert_with(Sink::new);
        let _ = sink.tx.send(frame);
    }

    /// Take `link`'s settlement receiver for the far-end to merge with the base
    /// broadcast. First subscription returns the receiver created with the
    /// channel; a reconnect (the prior receiver was taken and dropped) recreates
    /// the channel so future settlements drain to the fresh receiver. `now`
    /// clears any pending expiry.
    pub fn subscribe(&self, link: &LinkId, now: u64) -> mpsc::UnboundedReceiver<Frame> {
        let mut sinks = self.lock();
        let sink = sinks.entry(link.clone()).or_insert_with(Sink::new);
        sink.dead_since = None;
        let _ = now;
        match sink.rx.take() {
            Some(rx) => rx,
            None => {
                // Reconnect: recreate the channel. The prior sender (with any
                // buffered disconnect-window settlements) is dropped.
                let (tx, rx) = mpsc::unbounded_channel();
                sink.tx = tx;
                rx
            }
        }
    }

    /// Drop sinks whose subscriber has been gone for longer than the TTL, driven
    /// by the explicit `now` tick. Returns the number reaped (for observability
    /// and tests). A sink first observed dead at `now` starts its countdown then;
    /// a reconnect before `now - dead_since > ttl` clears it.
    pub fn reap(&self, now: u64) -> usize {
        let mut sinks = self.lock();
        let ttl = self.ttl;
        let mut reaped = 0;
        sinks.retain(|_, sink| {
            if sink.subscriber_gone() {
                match sink.dead_since {
                    None => {
                        sink.dead_since = Some(now);
                        true
                    }
                    Some(since) if now.saturating_sub(since) > ttl => {
                        reaped += 1;
                        false
                    }
                    Some(_) => true,
                }
            } else {
                sink.dead_since = None;
                true
            }
        });
        reaped
    }

    /// The number of live sinks (for tests/observability).
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether the store holds no sinks.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }
}

impl<LinkId, Frame> Default for SettlementSinkStore<LinkId, Frame>
where
    LinkId: Clone + Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settlement_routes_only_to_the_originating_link() {
        let store: SettlementSinkStore<&str, u32> = SettlementSinkStore::new();
        store.emit(&"a", 1);
        let mut rx_a = store.subscribe(&"a", 0);
        let mut rx_b = store.subscribe(&"b", 0);
        assert_eq!(rx_a.try_recv().ok(), Some(1), "a receives its settlement");
        assert!(rx_b.try_recv().is_err(), "b must not receive a's settlement");
    }

    #[test]
    fn a_reconnecting_subscriber_resumes_its_stream() {
        let store: SettlementSinkStore<&str, u32> = SettlementSinkStore::new();
        let first = store.subscribe(&"a", 0);
        drop(first);
        let mut second = store.subscribe(&"a", 1);
        store.emit(&"a", 42);
        assert_eq!(second.try_recv().ok(), Some(42));
    }

    #[test]
    fn a_settlement_emitted_before_subscribe_is_buffered() {
        let store: SettlementSinkStore<&str, u32> = SettlementSinkStore::new();
        store.emit(&"a", 7);
        let mut rx = store.subscribe(&"a", 0);
        assert_eq!(rx.try_recv().ok(), Some(7));
    }

    #[test]
    fn reaper_drops_a_sink_whose_subscriber_stayed_gone_past_the_ttl() {
        let store: SettlementSinkStore<&str, u32> = SettlementSinkStore::with_ttl(10);
        let rx = store.subscribe(&"a", 0);
        drop(rx); // subscriber vanished
        assert_eq!(store.reap(5), 0, "first observation starts the countdown");
        assert_eq!(store.reap(12), 0, "still within ttl (12 - 5 = 7 <= 10)");
        assert_eq!(store.reap(20), 1, "20 - 5 = 15 > 10 → reaped");
        assert!(store.is_empty());
    }

    #[test]
    fn reaper_spares_a_reconnected_subscriber() {
        let store: SettlementSinkStore<&str, u32> = SettlementSinkStore::with_ttl(10);
        let rx = store.subscribe(&"a", 0);
        drop(rx);
        store.reap(5); // countdown starts
        let _rx2 = store.subscribe(&"a", 8); // reconnect clears dead_since
        assert_eq!(store.reap(100), 0, "a live subscriber is never reaped");
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn reaper_spares_a_never_subscribed_sink_with_buffered_settlements() {
        let store: SettlementSinkStore<&str, u32> = SettlementSinkStore::with_ttl(1);
        store.emit(&"a", 1); // buffered, no subscriber yet
        assert_eq!(store.reap(1000), 0, "buffered settlements are not reaped");
        assert_eq!(store.len(), 1);
    }
}
