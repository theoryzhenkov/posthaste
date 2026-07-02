//! The idempotency-dedup sub-store (RFC D45/D47/D48).
//!
//! One keyed ledger of `(LinkId, ClientMutationId)` → a caller-owned record,
//! shared by both far-ends: a near node dedups its clients' mutations; the
//! authority server dedups the runtimes' forwarded mutations
//! ([replication authority-server-link L1 §3.1](../replication/authority-server-link/L1.md)).
//! `accept` atomically reserves a pending slot so a concurrent retry cannot
//! double-apply; `settle` records the terminal outcome under the **D47
//! terminal-class rule**:
//!
//! - [`TerminalClass::Confirmed`] — success. The record is kept so a duplicate
//!   re-observes the confirmation.
//! - [`TerminalClass::Rejected`] — a permanent verdict (validation/authz). The
//!   record is kept so a duplicate returns the same verdict and never re-executes.
//! - [`TerminalClass::Failed`] — a transient execution error. The record is
//!   **cleared** on settlement, so a deliberate retry re-accepts as
//!   [`Accept::New`] and re-executes.
//!
//! **Retention is time-and-acknowledgment bounded, not count bounded (D48).** A
//! kept terminal record (Confirmed OR Rejected, uniformly) evicts when either:
//!
//! - (a) the subscriber's **resume cursor** has passed the settlement frame's seq
//!   ([`ack`](DedupStore::ack), fed from the replay store's resume calls — the ack
//!   signal M9a already tracks), i.e. the client has demonstrably seen the verdict; or
//! - (b) its **age** exceeds a tick-driven TTL ([`reap`](DedupStore::reap),
//!   reusing the sink reaper's tick), which must dominate the near-end engine's
//!   retry horizon (4 attempts / 30s cap) so a retry never outlives the record.
//!
//! A generous per-link hard cap ([`DEFAULT_TERMINAL_CAPACITY`]) remains only as a
//! flood safety valve. This replaces the M9a per-class count windows and the
//! M9b2 Rejected-cap knob: retention protects retry-dedup and verdict
//! re-observation, both bounded by time-and-acknowledgment, not count (a count
//! cap breaks dedup under burst — when retries are likeliest — and hoards when
//! idle). Pending records are never evicted.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Mutex;

use posthaste_contract_core::ClientMutationId;

/// Generous per-link **safety-valve** cap on retained terminal records (Confirmed
/// AND Rejected, uniform — D48). This is not the operative retention bound —
/// acked-cursor eviction and the TTL are — only a flood ceiling so a pathological
/// burst cannot grow the ledger without limit. Sized thousands, not the M9a `100`.
pub const DEFAULT_TERMINAL_CAPACITY: usize = 4096;

/// Default TTL for a terminal record, in the sink reaper's `now` tick units
/// (**seconds** as the authority server drives it) — `900` = fifteen minutes.
/// It must dominate the near-end engine's retry horizon (4 attempts, 30s backoff
/// cap ⇒ ~2 min worst case) so a deliberate retry can never outlive the record
/// that would dedup it; acked-cursor eviction reclaims sooner in the common case.
pub const DEFAULT_TERMINAL_TTL: u64 = 900;

/// The terminal class of a settled mutation (D47) — the keep-vs-clear verdict
/// the assembling far-end derives from its own settlement state + error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalClass {
    /// Success. The record is kept (D48 retention), so a duplicate re-observes it.
    Confirmed,
    /// A permanent verdict (validation/authz). The record is kept; a duplicate
    /// re-observes it, never re-executes.
    Rejected,
    /// A transient execution error. The record is cleared on settlement; a
    /// deliberate retry re-accepts as [`Accept::New`] and re-executes.
    Failed,
}

/// The outcome of [`DedupStore::accept`].
pub enum Accept<R> {
    /// First time this `(link, client_mutation_id)` was seen: a pending record
    /// was reserved. The caller executes the effect, then calls
    /// [`DedupStore::settle`].
    New,
    /// The key is already known — an in-flight pending record or a kept terminal
    /// verdict. The caller returns this record's verdict without re-executing.
    Duplicate(R),
}

/// A kept terminal outcome (D48): the class, the replay seq of the settlement
/// frame this seam emitted for it (for acked-cursor eviction), and the tick it
/// settled at (for the TTL fallback).
#[derive(Clone, Copy)]
struct Terminal {
    class: TerminalClass,
    /// The replay seq of the emitted settlement frame, if any. `None` when this
    /// seam emits no down-frame for the class (the AS seam's Rejected — the near
    /// node learns of it via the up-channel error, not a settlement frame), so
    /// acked-cursor eviction never fires for it and the TTL/cap govern alone.
    settlement_seq: Option<u64>,
    /// The reaper tick at which it settled — drives the TTL fallback.
    settled_at: u64,
}

#[derive(Clone, Copy)]
enum Status {
    Pending,
    Terminal(Terminal),
}

struct Entry<R> {
    record: R,
    status: Status,
}

struct LinkLedger<R> {
    entries: HashMap<ClientMutationId, Entry<R>>,
    /// Terminal keys (Confirmed AND Rejected, uniform — D48) in settlement order,
    /// for the safety-valve cap. Pending and cleared `Failed` never appear.
    terminal_order: VecDeque<ClientMutationId>,
}

impl<R> LinkLedger<R> {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            terminal_order: VecDeque::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop a key from the ledger (entry + settlement-order bookkeeping).
    fn drop_key(&mut self, key: &ClientMutationId) {
        self.entries.remove(key);
        self.terminal_order.retain(|k| k != key);
    }
}

/// The shared idempotency-dedup ledger, generic over the seam's `LinkId` and the
/// caller-owned record `R`. Keyed `(LinkId, ClientMutationId)`; retention is
/// per-link, time-and-acknowledgment bounded (D48).
pub struct DedupStore<LinkId, R> {
    links: Mutex<HashMap<LinkId, LinkLedger<R>>>,
    /// Per-link safety-valve cap (D48) — not the operative bound.
    capacity: usize,
    /// Terminal-record TTL in reaper ticks (D48).
    ttl: u64,
}

impl<LinkId, R> DedupStore<LinkId, R>
where
    LinkId: Clone + Eq + Hash,
    R: Clone,
{
    /// A store with the default safety-valve cap and TTL (D48).
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_TERMINAL_CAPACITY)
    }

    /// A store with an explicit per-link safety-valve cap and the default TTL.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            links: Mutex::new(HashMap::new()),
            capacity: capacity.max(1),
            ttl: DEFAULT_TERMINAL_TTL,
        }
    }

    /// Override the terminal-record TTL (reaper ticks).
    pub fn with_ttl(mut self, ttl: u64) -> Self {
        self.ttl = ttl;
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<LinkId, LinkLedger<R>>> {
        self.links.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Reserve a pending slot for `(link, client_mutation_id)` if absent,
    /// building the record from `make`; otherwise return a clone of the stored
    /// record so the caller can dedup against it. The lock is released before
    /// returning so the caller's `await` never holds it.
    pub fn accept(
        &self,
        link: &LinkId,
        client_mutation_id: &ClientMutationId,
        make: impl FnOnce() -> R,
    ) -> Accept<R> {
        let mut links = self.lock();
        let ledger = links.entry(link.clone()).or_insert_with(LinkLedger::new);
        if let Some(entry) = ledger.entries.get(client_mutation_id) {
            return Accept::Duplicate(entry.record.clone());
        }
        ledger.entries.insert(
            client_mutation_id.clone(),
            Entry {
                record: make(),
                status: Status::Pending,
            },
        );
        Accept::New
    }

    /// Settle a reserved record under the D47 terminal-class rule with D48
    /// retention. `update` writes the verdict payload into the stored record
    /// before the class rule applies (for `Failed` it is not called — the record
    /// is cleared). `settlement_seq` is the replay seq of the settlement frame
    /// emitted for this mutation (the ack target), or `None` when this seam emits
    /// none for the class. `now` is the reaper tick the TTL counts from. A
    /// missing key is a no-op (already evicted or cleared).
    pub fn settle(
        &self,
        link: &LinkId,
        client_mutation_id: &ClientMutationId,
        class: TerminalClass,
        settlement_seq: Option<u64>,
        now: u64,
        update: impl FnOnce(&mut R),
    ) {
        let mut links = self.lock();
        let Some(ledger) = links.get_mut(link) else {
            return;
        };
        match class {
            TerminalClass::Failed => {
                // Transient: clear the record so a deliberate retry re-executes.
                ledger.drop_key(client_mutation_id);
            }
            // Confirmed and Rejected are retained uniformly (D48): stamped with
            // the settlement seq + tick, subject to acked-cursor / TTL / cap
            // eviction all the same. The only D47 distinction is keep-vs-clear
            // (both keep) and what a duplicate re-observes.
            TerminalClass::Confirmed | TerminalClass::Rejected => {
                if let Some(entry) = ledger.entries.get_mut(client_mutation_id) {
                    update(&mut entry.record);
                    entry.status = Status::Terminal(Terminal {
                        class,
                        settlement_seq,
                        settled_at: now,
                    });
                    ledger.terminal_order.push_back(client_mutation_id.clone());
                    prune_capacity(ledger, self.capacity);
                }
            }
        }
        if ledger.is_empty() {
            links.remove(link);
        }
    }

    /// Acked-cursor eviction (D48 (a)): the subscriber's resume cursor passed
    /// `cursor`, so every kept terminal whose settlement frame the subscriber has
    /// now demonstrably seen (`settlement_seq <= cursor`) is reclaimed. Wired from
    /// the replay store's resume calls — resume(after_seq) IS the ack.
    pub fn ack(&self, link: &LinkId, cursor: u64) {
        let mut links = self.lock();
        let Some(ledger) = links.get_mut(link) else {
            return;
        };
        let evict: Vec<ClientMutationId> = ledger
            .entries
            .iter()
            .filter_map(|(key, entry)| match entry.status {
                Status::Terminal(Terminal {
                    settlement_seq: Some(seq),
                    ..
                }) if seq <= cursor => Some(key.clone()),
                _ => None,
            })
            .collect();
        for key in evict {
            ledger.drop_key(&key);
        }
        if ledger.is_empty() {
            links.remove(link);
        }
    }

    /// TTL fallback eviction (D48 (b)): drop every terminal record older than the
    /// TTL, driven by the explicit `now` tick (the sink reaper's tick — reused, so
    /// dedup TTL and sink expiry share one cadence). Returns the number reaped.
    pub fn reap(&self, now: u64) -> usize {
        let mut links = self.lock();
        let ttl = self.ttl;
        let mut reaped = 0;
        links.retain(|_, ledger| {
            let expired: Vec<ClientMutationId> = ledger
                .entries
                .iter()
                .filter_map(|(key, entry)| match entry.status {
                    Status::Terminal(Terminal { settled_at, .. })
                        if now.saturating_sub(settled_at) >= ttl =>
                    {
                        Some(key.clone())
                    }
                    _ => None,
                })
                .collect();
            for key in expired {
                ledger.drop_key(&key);
                reaped += 1;
            }
            !ledger.is_empty()
        });
        reaped
    }

    /// Drop a reserved pending record without recording any verdict — an atomic
    /// apply that failed before producing a terminal outcome, so a retry
    /// re-accepts as [`Accept::New`]. (Equivalent to `settle(_, Failed, ..)`
    /// with no payload; kept as a named entry point for the atomic-apply path.)
    pub fn clear(&self, link: &LinkId, client_mutation_id: &ClientMutationId) {
        let mut links = self.lock();
        if let Some(ledger) = links.get_mut(link) {
            ledger.drop_key(client_mutation_id);
            if ledger.is_empty() {
                links.remove(link);
            }
        }
    }

    /// The stored record for a key, or `None` when unknown (never accepted,
    /// evicted, or cleared).
    pub fn verdict(&self, link: &LinkId, client_mutation_id: &ClientMutationId) -> Option<R> {
        let links = self.lock();
        links
            .get(link)?
            .entries
            .get(client_mutation_id)
            .map(|entry| entry.record.clone())
    }

    /// The terminal class of a key, or `None` when unknown or still pending.
    pub fn terminal_class(
        &self,
        link: &LinkId,
        client_mutation_id: &ClientMutationId,
    ) -> Option<TerminalClass> {
        let links = self.lock();
        match links.get(link)?.entries.get(client_mutation_id)?.status {
            Status::Terminal(terminal) => Some(terminal.class),
            Status::Pending => None,
        }
    }

    /// A clone of every record currently held for a link (any status), for the
    /// far-end's collapse/replay enumeration. Order is unspecified; the caller
    /// sorts.
    pub fn records_for(&self, link: &LinkId) -> Vec<R> {
        let links = self.lock();
        links
            .get(link)
            .map(|ledger| ledger.entries.values().map(|e| e.record.clone()).collect())
            .unwrap_or_default()
    }

    /// Drop every record for a link — the far-end's teardown when the link
    /// closes (a runtime session ends / a runtime departs — the sink reaper's
    /// departure purge, [6]).
    pub fn purge(&self, link: &LinkId) {
        self.lock().remove(link);
    }
}

impl<LinkId, R> Default for DedupStore<LinkId, R>
where
    LinkId: Clone + Eq + Hash,
    R: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Enforce the per-link safety-valve cap (D48): once a link's terminal window
/// exceeds the cap, evict the oldest terminals (settlement order). This is the
/// flood ceiling only — acked-cursor / TTL eviction do the real reclaiming.
fn prune_capacity<R>(ledger: &mut LinkLedger<R>, capacity: usize) {
    while ledger.terminal_order.len() > capacity {
        let Some(oldest) = ledger.terminal_order.pop_front() else {
            break;
        };
        // Only evict if still terminal (defensive against a re-push after a
        // Failed clear + re-accept + re-settle of the same key).
        if matches!(
            ledger.entries.get(&oldest).map(|e| e.status),
            Some(Status::Terminal(_))
        ) {
            ledger.entries.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cid(s: &str) -> ClientMutationId {
        ClientMutationId::new(s)
    }

    // Convenience: settle Confirmed with a settlement seq at tick 0.
    fn settle_confirmed<L: Clone + Eq + Hash>(
        store: &DedupStore<L, String>,
        link: &L,
        c: &ClientMutationId,
        seq: u64,
    ) {
        store.settle(link, c, TerminalClass::Confirmed, Some(seq), 0, |_| {});
    }

    // D47: a Failed (transient) terminal CLEARS the record, so a deliberate
    // retry re-accepts as New and re-executes.
    #[test]
    fn retry_after_failed_re_executes() {
        let store: DedupStore<&str, String> = DedupStore::new();
        assert!(matches!(
            store.accept(&"link", &cid("op-1"), || "verdict-a".to_string()),
            Accept::New
        ));
        store.settle(&"link", &cid("op-1"), TerminalClass::Failed, None, 0, |_| {});
        assert!(store.verdict(&"link", &cid("op-1")).is_none());
        assert!(matches!(
            store.accept(&"link", &cid("op-1"), || "verdict-b".to_string()),
            Accept::New
        ));
    }

    // D47: a Rejected (permanent) terminal KEEPS the record, so a duplicate
    // returns the stored verdict and never re-executes.
    #[test]
    fn retry_after_rejected_returns_stored_verdict() {
        let store: DedupStore<&str, String> = DedupStore::new();
        assert!(matches!(
            store.accept(&"link", &cid("op-1"), || "pending".to_string()),
            Accept::New
        ));
        store.settle(&"link", &cid("op-1"), TerminalClass::Rejected, Some(1), 0, |record| {
            *record = "rejected-verdict".to_string();
        });
        match store.accept(&"link", &cid("op-1"), || "should-not-run".to_string()) {
            Accept::Duplicate(record) => assert_eq!(record, "rejected-verdict"),
            Accept::New => panic!("a Rejected duplicate must not re-accept"),
        }
        assert_eq!(
            store.terminal_class(&"link", &cid("op-1")),
            Some(TerminalClass::Rejected)
        );
    }

    // Pending dedup is unchanged: a duplicate while in flight returns the
    // reserved (pending) record.
    #[test]
    fn pending_dedup_returns_the_in_flight_record() {
        let store: DedupStore<&str, String> = DedupStore::new();
        assert!(matches!(
            store.accept(&"link", &cid("op-1"), || "in-flight".to_string()),
            Accept::New
        ));
        match store.accept(&"link", &cid("op-1"), || "duplicate".to_string()) {
            Accept::Duplicate(record) => assert_eq!(record, "in-flight"),
            Accept::New => panic!("a pending duplicate must not re-accept"),
        }
        assert!(store.terminal_class(&"link", &cid("op-1")).is_none());
    }

    // D48 (a): a terminal record survives until the subscriber's resume cursor
    // passes its settlement seq — verdict re-observation guaranteed until ack,
    // then reclaimed. Uniform for Confirmed AND Rejected.
    #[test]
    fn acked_cursor_evicts_terminals_it_has_passed() {
        let store: DedupStore<&str, String> = DedupStore::new();
        for (op, seq) in [("op-1", 3u64), ("op-2", 5), ("op-3", 9)] {
            store.accept(&"link", &cid(op), || String::new());
            settle_confirmed(&store, &"link", &cid(op), seq);
        }
        // Ack up to seq 5: op-1 (seq 3) and op-2 (seq 5) are seen and reclaimed;
        // op-3 (seq 9) is not yet acked and is still re-observable.
        store.ack(&"link", 5);
        assert!(store.verdict(&"link", &cid("op-1")).is_none(), "acked, reclaimed");
        assert!(store.verdict(&"link", &cid("op-2")).is_none(), "acked (== cursor)");
        assert!(store.verdict(&"link", &cid("op-3")).is_some(), "not yet acked");
    }

    // D48 (a) uniform: a Rejected verdict with a settlement seq is acked-evicted
    // just like a Confirmed one (the runtime seam emits a notification frame for
    // rejections, so they carry a seq).
    #[test]
    fn acked_cursor_evicts_rejected_too() {
        let store: DedupStore<&str, String> = DedupStore::new();
        store.accept(&"link", &cid("rej"), || String::new());
        store.settle(&"link", &cid("rej"), TerminalClass::Rejected, Some(4), 0, |r| {
            *r = "no".into()
        });
        store.ack(&"link", 4);
        assert!(store.verdict(&"link", &cid("rej")).is_none(), "acked rejection reclaimed");
    }

    // D48: a Rejected with no settlement frame (settlement_seq None — the AS
    // seam) is never acked-evicted; only TTL/cap reclaim it.
    #[test]
    fn a_frameless_rejection_survives_acks_until_ttl() {
        let store: DedupStore<&str, String> = DedupStore::with_capacity(4096).with_ttl(10);
        store.accept(&"link", &cid("rej"), || String::new());
        store.settle(&"link", &cid("rej"), TerminalClass::Rejected, None, 100, |r| {
            *r = "no".into()
        });
        // No cursor can ack a frameless rejection.
        store.ack(&"link", u64::MAX);
        assert!(store.verdict(&"link", &cid("rej")).is_some(), "no seq → ack cannot evict");
        // Within TTL it survives; past TTL it is reaped.
        assert_eq!(store.reap(105), 0, "within ttl");
        assert!(store.verdict(&"link", &cid("rej")).is_some());
        assert_eq!(store.reap(111), 1, "past ttl (111 - 100 >= 10)");
        assert!(store.verdict(&"link", &cid("rej")).is_none());
    }

    // D48 (b): the TTL fallback reaps stale terminals uniformly.
    #[test]
    fn ttl_reaps_stale_terminals() {
        let store: DedupStore<&str, String> = DedupStore::with_capacity(4096).with_ttl(10);
        store.accept(&"link", &cid("op"), || String::new());
        store.settle(&"link", &cid("op"), TerminalClass::Confirmed, Some(1), 100, |_| {});
        assert_eq!(store.reap(109), 0, "within ttl");
        assert_eq!(store.reap(110), 1, "at ttl boundary (110 - 100 >= 10)");
        assert!(store.verdict(&"link", &cid("op")).is_none());
    }

    // Pending records are never touched by ack, TTL, or the cap.
    #[test]
    fn pending_is_never_evicted() {
        let store: DedupStore<&str, u64> = DedupStore::with_capacity(2).with_ttl(1);
        store.accept(&"link", &cid("pending"), || 999);
        for i in 0..10u64 {
            let c = cid(&format!("cf-{i}"));
            store.accept(&"link", &c, || i);
            store.settle(&"link", &c, TerminalClass::Confirmed, Some(i + 1), 0, |_| {});
        }
        store.ack(&"link", u64::MAX);
        store.reap(u64::MAX);
        assert_eq!(store.verdict(&"link", &cid("pending")), Some(999));
    }

    // The safety-valve cap bounds a flood: past the cap the oldest terminals fall
    // out (settlement order), but the cap is generous, not the operative bound.
    #[test]
    fn safety_valve_cap_bounds_a_flood() {
        let store: DedupStore<&str, u64> = DedupStore::with_capacity(4);
        for i in 0..9u64 {
            let c = cid(&format!("op-{i}"));
            store.accept(&"link", &c, || i);
            store.settle(&"link", &c, TerminalClass::Confirmed, Some(i + 1), 0, |_| {});
        }
        assert!(store.verdict(&"link", &cid("op-0")).is_none(), "oldest flooded out");
        assert!(store.verdict(&"link", &cid("op-8")).is_some(), "newest kept");
        assert!(store.records_for(&"link").len() <= 4, "cap bounds the window");
    }

    #[test]
    fn per_link_ledgers_are_independent() {
        let store: DedupStore<&str, u64> = DedupStore::with_capacity(4096).with_ttl(10);
        store.accept(&"a", &cid("a-0"), || 0);
        store.settle(&"a", &cid("a-0"), TerminalClass::Confirmed, Some(1), 100, |_| {});
        store.accept(&"b", &cid("b-0"), || 0);
        store.settle(&"b", &cid("b-0"), TerminalClass::Confirmed, Some(1), 100, |_| {});
        // Acking link "a" leaves link "b" untouched.
        store.ack(&"a", 1);
        assert!(store.verdict(&"a", &cid("a-0")).is_none());
        assert!(store.verdict(&"b", &cid("b-0")).is_some());
    }

    #[test]
    fn clear_drops_a_reserved_pending_for_atomic_apply_failure() {
        let store: DedupStore<&str, u64> = DedupStore::new();
        store.accept(&"link", &cid("op"), || 1);
        store.clear(&"link", &cid("op"));
        assert!(matches!(store.accept(&"link", &cid("op"), || 2), Accept::New));
    }

    #[test]
    fn purge_drops_every_record_for_a_link() {
        let store: DedupStore<&str, u64> = DedupStore::new();
        store.accept(&"link", &cid("a"), || 1);
        store.accept(&"link", &cid("b"), || 2);
        store.purge(&"link");
        assert!(store.records_for(&"link").is_empty());
    }
}
