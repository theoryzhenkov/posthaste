//! The idempotency-dedup sub-store (RFC D45/D47).
//!
//! One keyed ledger of `(LinkId, ClientMutationId)` → a caller-owned record,
//! shared by both far-ends: a near node dedups its clients' mutations; the
//! authority server dedups the runtimes' forwarded mutations
//! ([replication authority-server-link L1 §3.1](../replication/authority-server-link/L1.md)).
//! `accept` atomically reserves a pending slot so a concurrent retry cannot
//! double-apply; `settle` records the terminal outcome under the **D47
//! terminal-class rule**:
//!
//! - [`TerminalClass::Confirmed`] — success. The record is kept and joins the
//!   per-link bounded eviction window ([`DEFAULT_TERMINAL_CAPACITY`]).
//! - [`TerminalClass::Rejected`] — a permanent verdict (validation/authz). The
//!   record is kept and exempt from the eviction window, so a reconnecting
//!   subscriber always re-observes it; a duplicate `ClientMutationId` returns
//!   the same verdict and never re-executes.
//! - [`TerminalClass::Failed`] — a transient execution error. The record is
//!   **cleared** on settlement, so a deliberate retry re-accepts as
//!   [`Accept::New`] and re-executes.
//!
//! This split (D47) fixes both seams' former half-wrong behavior in one place:
//! the runtime seam used to keep transient failures (a retry deduped into the
//! stale failure), and the authority server seam used to clear everything (a
//! retry re-executed a permanent rejection). Pending records are never evicted.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Mutex;

use posthaste_contract_core::ClientMutationId;

/// Default bound on retained [`TerminalClass::Confirmed`] records **per link**,
/// preserving the cap the two hand-rolled far-ends each used (`100`). Bounds
/// reconnect/dedup memory rather than letting it grow with a link's age.
pub const DEFAULT_TERMINAL_CAPACITY: usize = 100;

/// Default per-link bound on retained [`TerminalClass::Rejected`] records for
/// an assembly that opts into one ([`DedupStore::with_rejected_capacity`]).
/// The V14 follow-up knob: the AS seam's links live for a runtime's whole
/// uptime, so its Rejected ledger is bounded here; the runtime seam leaves it
/// unbounded (a client session's lifetime bounds it naturally, and a stranded
/// client must always be able to re-observe its rejection verdicts).
pub const DEFAULT_REJECTED_CAPACITY: usize = 100;

/// The terminal class of a settled mutation (D47) — the keep-vs-clear verdict
/// the assembling far-end derives from its own settlement state + error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalClass {
    /// Success. The record is kept and bounded-evicted (per-link window).
    Confirmed,
    /// A permanent verdict (validation/authz). The record is kept and exempt
    /// from the eviction window; a duplicate re-observes it, never re-executes.
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Pending,
    Terminal(TerminalClass),
}

struct Entry<R> {
    record: R,
    status: Status,
}

struct LinkLedger<R> {
    entries: HashMap<ClientMutationId, Entry<R>>,
    /// `Confirmed` terminals in settlement order, for bounded eviction. Pending,
    /// `Rejected`, and (already-removed) `Failed` records never appear here.
    confirmed_order: VecDeque<ClientMutationId>,
    /// `Rejected` terminals in settlement order — populated (and pruned) only
    /// when the assembly bounds its Rejected window
    /// ([`DedupStore::with_rejected_capacity`]).
    rejected_order: VecDeque<ClientMutationId>,
}

impl<R> LinkLedger<R> {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            confirmed_order: VecDeque::new(),
            rejected_order: VecDeque::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The shared idempotency-dedup ledger, generic over the seam's `LinkId` and the
/// caller-owned record `R`. Keyed `(LinkId, ClientMutationId)`; eviction is
/// per-link so one busy link never evicts another's window.
pub struct DedupStore<LinkId, R> {
    links: Mutex<HashMap<LinkId, LinkLedger<R>>>,
    capacity: usize,
    /// Per-link bound on retained `Rejected` terminals. `None` = unbounded
    /// (the runtime seam: the link is a client session, whose lifetime bounds
    /// the ledger); `Some` = a settlement-order eviction window (the AS seam:
    /// links live for a runtime's uptime — V14 follow-up).
    rejected_capacity: Option<usize>,
}

impl<LinkId, R> DedupStore<LinkId, R>
where
    LinkId: Clone + Eq + Hash,
    R: Clone,
{
    /// A store with the default per-link `Confirmed` retention cap
    /// ([`DEFAULT_TERMINAL_CAPACITY`]) and an unbounded `Rejected` window.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_TERMINAL_CAPACITY)
    }

    /// A store with an explicit per-link `Confirmed` retention cap and an
    /// unbounded `Rejected` window.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            links: Mutex::new(HashMap::new()),
            capacity,
            rejected_capacity: None,
        }
    }

    /// Bound the per-link `Rejected` retention window (settlement-order
    /// eviction, like the `Confirmed` window). For assemblies whose links
    /// outlive any client session (the AS seam); leaving it unbounded is
    /// correct where the link's own lifetime bounds the ledger.
    pub fn with_rejected_capacity(mut self, capacity: usize) -> Self {
        self.rejected_capacity = Some(capacity);
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

    /// Settle a reserved record under the D47 terminal-class rule. `update`
    /// writes the verdict payload into the stored record before the class rule
    /// applies (for `Failed` it is not called — the record is cleared). A
    /// missing key is a no-op (already evicted or cleared).
    pub fn settle(
        &self,
        link: &LinkId,
        client_mutation_id: &ClientMutationId,
        class: TerminalClass,
        update: impl FnOnce(&mut R),
    ) {
        let mut links = self.lock();
        let Some(ledger) = links.get_mut(link) else {
            return;
        };
        match class {
            TerminalClass::Failed => {
                // Transient: clear the record so a deliberate retry re-executes.
                ledger.entries.remove(client_mutation_id);
            }
            TerminalClass::Rejected => {
                if let Some(entry) = ledger.entries.get_mut(client_mutation_id) {
                    update(&mut entry.record);
                    entry.status = Status::Terminal(TerminalClass::Rejected);
                    // Kept and exempt from the `Confirmed` window (recovery
                    // path); an assembly may bound Rejected retention with its
                    // own window (V14 follow-up knob).
                    if let Some(capacity) = self.rejected_capacity {
                        ledger.rejected_order.push_back(client_mutation_id.clone());
                        prune_rejected(ledger, capacity);
                    }
                }
            }
            TerminalClass::Confirmed => {
                let Some(entry) = ledger.entries.get_mut(client_mutation_id) else {
                    return;
                };
                update(&mut entry.record);
                entry.status = Status::Terminal(TerminalClass::Confirmed);
                ledger.confirmed_order.push_back(client_mutation_id.clone());
                prune_confirmed(ledger, self.capacity);
            }
        }
        if ledger.is_empty() {
            links.remove(link);
        }
    }

    /// Drop a reserved pending record without recording any verdict — an atomic
    /// apply that failed before producing a terminal outcome, so a retry
    /// re-accepts as [`Accept::New`]. (Equivalent to `settle(_, Failed, ..)`
    /// with no payload; kept as a named entry point for the atomic-apply path.)
    pub fn clear(&self, link: &LinkId, client_mutation_id: &ClientMutationId) {
        let mut links = self.lock();
        if let Some(ledger) = links.get_mut(link) {
            ledger.entries.remove(client_mutation_id);
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
            Status::Terminal(class) => Some(class),
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
    /// closes (e.g. a runtime session ends).
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

/// Evict the oldest `Confirmed` terminals once a link's window exceeds the cap.
/// Pending / `Rejected` records never enter `confirmed_order`, so they are never
/// touched here; an entry already removed (raced) is skipped.
fn prune_confirmed<R>(ledger: &mut LinkLedger<R>, capacity: usize) {
    while ledger.confirmed_order.len() > capacity {
        let Some(oldest) = ledger.confirmed_order.pop_front() else {
            break;
        };
        // Only evict if still a Confirmed terminal (defensive against a re-push).
        if matches!(
            ledger.entries.get(&oldest).map(|e| e.status),
            Some(Status::Terminal(TerminalClass::Confirmed))
        ) {
            ledger.entries.remove(&oldest);
        }
    }
}

/// Evict the oldest `Rejected` terminals once a link's bounded Rejected window
/// exceeds its cap (only assemblies that opted in via
/// [`DedupStore::with_rejected_capacity`] ever populate `rejected_order`).
fn prune_rejected<R>(ledger: &mut LinkLedger<R>, capacity: usize) {
    while ledger.rejected_order.len() > capacity {
        let Some(oldest) = ledger.rejected_order.pop_front() else {
            break;
        };
        // Only evict if still a Rejected terminal (a Failed retry may have
        // cleared + re-accepted the key since).
        if matches!(
            ledger.entries.get(&oldest).map(|e| e.status),
            Some(Status::Terminal(TerminalClass::Rejected))
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

    // D47: a Failed (transient) terminal CLEARS the record, so a deliberate
    // retry re-accepts as New and re-executes.
    #[test]
    fn retry_after_failed_re_executes() {
        let store: DedupStore<&str, String> = DedupStore::new();
        assert!(matches!(
            store.accept(&"link", &cid("op-1"), || "verdict-a".to_string()),
            Accept::New
        ));
        store.settle(&"link", &cid("op-1"), TerminalClass::Failed, |_| {});
        // The record is gone: the query returns None and a retry re-accepts New.
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
        store.settle(&"link", &cid("op-1"), TerminalClass::Rejected, |record| {
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

    // Pending dedup is unchanged by D47: a duplicate while in flight returns the
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

    #[test]
    fn confirmed_terminals_evict_per_link_at_capacity() {
        let store: DedupStore<&str, u64> = DedupStore::with_capacity(4);
        for i in 0..9u64 {
            let c = cid(&format!("op-{i}"));
            store.accept(&"link", &c, || i);
            store.settle(&"link", &c, TerminalClass::Confirmed, |_| {});
        }
        assert!(store.verdict(&"link", &cid("op-0")).is_none(), "oldest evicted");
        assert!(store.verdict(&"link", &cid("op-8")).is_some(), "newest kept");
        let held = store.records_for(&"link").len();
        assert!(held <= 4, "per-link window bounded, got {held}");
    }

    #[test]
    fn per_link_windows_are_independent() {
        let store: DedupStore<&str, u64> = DedupStore::with_capacity(2);
        for link in ["a", "b"] {
            for i in 0..2u64 {
                let c = cid(&format!("{link}-{i}"));
                store.accept(&link, &c, || i);
                store.settle(&link, &c, TerminalClass::Confirmed, |_| {});
            }
        }
        // Filling link "a" past its cap must not evict link "b".
        for i in 2..5u64 {
            let c = cid(&format!("a-{i}"));
            store.accept(&"a", &c, || i);
            store.settle(&"a", &c, TerminalClass::Confirmed, |_| {});
        }
        assert!(store.verdict(&"b", &cid("b-0")).is_some());
        assert!(store.verdict(&"b", &cid("b-1")).is_some());
    }

    // The V14 follow-up knob: a bounded Rejected window evicts oldest-first,
    // independently of the Confirmed window; unbounded (default) keeps all.
    #[test]
    fn bounded_rejected_window_evicts_oldest_first() {
        let store: DedupStore<&str, u64> = DedupStore::with_capacity(100).with_rejected_capacity(2);
        for i in 0..5u64 {
            let c = cid(&format!("rej-{i}"));
            store.accept(&"link", &c, || i);
            store.settle(&"link", &c, TerminalClass::Rejected, |_| {});
        }
        assert!(store.verdict(&"link", &cid("rej-0")).is_none(), "oldest evicted");
        assert!(store.verdict(&"link", &cid("rej-2")).is_none(), "next-oldest evicted");
        assert_eq!(store.verdict(&"link", &cid("rej-3")), Some(3));
        assert_eq!(store.verdict(&"link", &cid("rej-4")), Some(4));
        // The Confirmed window is untouched by Rejected pruning.
        store.accept(&"link", &cid("cf"), || 99);
        store.settle(&"link", &cid("cf"), TerminalClass::Confirmed, |_| {});
        assert_eq!(store.verdict(&"link", &cid("cf")), Some(99));
    }

    #[test]
    fn rejected_is_exempt_from_the_confirmed_window() {
        let store: DedupStore<&str, &str> = DedupStore::with_capacity(3);
        store.accept(&"link", &cid("rej"), || "");
        store.settle(&"link", &cid("rej"), TerminalClass::Rejected, |r| *r = "verdict");
        for i in 0..20u64 {
            let c = cid(&format!("cf-{i}"));
            store.accept(&"link", &c, || "");
            store.settle(&"link", &c, TerminalClass::Confirmed, |_| {});
        }
        assert_eq!(
            store.verdict(&"link", &cid("rej")),
            Some("verdict"),
            "a Rejected verdict survives the Confirmed eviction window"
        );
    }

    #[test]
    fn pending_is_never_evicted() {
        let store: DedupStore<&str, u64> = DedupStore::with_capacity(2);
        store.accept(&"link", &cid("pending"), || 999);
        for i in 0..10u64 {
            let c = cid(&format!("cf-{i}"));
            store.accept(&"link", &c, || i);
            store.settle(&"link", &c, TerminalClass::Confirmed, |_| {});
        }
        assert_eq!(store.verdict(&"link", &cid("pending")), Some(999));
    }

    #[test]
    fn clear_drops_a_reserved_pending_for_atomic_apply_failure() {
        let store: DedupStore<&str, u64> = DedupStore::new();
        store.accept(&"link", &cid("op"), || 1);
        store.clear(&"link", &cid("op"));
        assert!(matches!(
            store.accept(&"link", &cid("op"), || 2),
            Accept::New
        ));
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
