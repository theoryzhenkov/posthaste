//! The backend far node's per-runtime registry
//! ([replication backend-link L1 §3.1](../replication/backend-link/L1.md)):
//! the `(RuntimeId, ClientMutationId)` → settled-mutation table that scopes
//! mutation-id idempotency and (in a later slice) settlement routing per runtime.
//!
//! Mirrors the runtime near node's own `mutations_by_client_id`
//! (`posthaste-runtime/src/sessions.rs`) one level up: a near node dedups its
//! clients' mutations; the backend dedups the runtimes' forwarded mutations.
//! `accept` atomically reserves a slot so a concurrent retry cannot double-apply;
//! the lock is never held across the mutation's `await`.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use posthaste_link_contract::{DownFrame, RuntimeId};
use posthaste_runtime_contract::{
    ClientMutationId, MutationReceipt, MutationSettlementState, RuntimeMutationId,
};
use serde_json::Value;
use tokio::sync::mpsc;

/// Upper bound on retained terminal mutations per registry, mirroring the near
/// node's `MAX_LATEST_MUTATIONS`. Bounds reconnect/dedup memory rather than
/// letting it grow with runtime age.
const MAX_RETAINED_MUTATIONS: usize = 100;

/// A forwarded mutation the backend has accepted, stored for `(RuntimeId,
/// ClientMutationId)` idempotency and (later) settlement routing.
#[derive(Clone)]
pub(crate) struct StoredForwardMutation {
    runtime_mutation_id: RuntimeMutationId,
    client_mutation_id: ClientMutationId,
    name: String,
    /// Serialized `CommandAck` — the receipt's `output`. `Null` while a just
    /// reserved entry is still applying (the co-located single-runtime path
    /// never observes this; multi-runtime reads it as a pending `Accepted`).
    output: Value,
    /// Whether the mutation has reached a terminal outcome (applied). Used only
    /// for bounded retention — the receipt's `state` is `Accepted` (the up-channel
    /// ack); `Confirmed`/`Failed` arrive on the down-channel in a later slice.
    settled: bool,
}

impl StoredForwardMutation {
    fn receipt(&self) -> MutationReceipt {
        MutationReceipt {
            runtime_mutation_id: Some(self.runtime_mutation_id.clone()),
            client_mutation_id: self.client_mutation_id.clone(),
            name: self.name.clone(),
            state: MutationSettlementState::Accepted,
            error: None,
            output: self.output.clone(),
        }
    }
}

/// The outcome of reserving a mutation at the backend up-channel.
pub(crate) enum ForwardAcceptance {
    /// First time this `(RuntimeId, ClientMutationId)` was seen: the backend
    /// assigned `RuntimeMutationId` and reserved a pending entry the caller
    /// must fill with `RuntimeRegistry::settle_output`.
    New { runtime_mutation_id: RuntimeMutationId },
    /// Already accepted: return the stored receipt (idempotent — never apply
    /// the user intent twice).
    Existing(MutationReceipt),
}

/// The per-runtime settlement-routing sink (`settlement-routed-to-origin-runtime`).
/// `tx` is held by the registry — `forward_mutation_for` emits a `DownFrame::Settlement`
/// on it once a mutation reaches its terminal outcome; `rx` is taken by
/// `subscribe_for` and merged with the broadcast `Base` stream onto that runtime's
/// down-stream. Unbounded so a Settlement emitted before the runtime subscribes
/// (or while its stream is briefly behind) is never dropped — a missed Settlement
/// would strand the near node's outbox entry. Reconnect/resume across a dropped
/// receiver is S4.
struct SettlementSink {
    tx: mpsc::UnboundedSender<DownFrame>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<DownFrame>>>,
}

impl SettlementSink {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            tx,
            rx: Mutex::new(Some(rx)),
        }
    }
}

pub(crate) struct RuntimeRegistry {
    mutations: Mutex<HashMap<(RuntimeId, ClientMutationId), StoredForwardMutation>>,
    /// Terminal mutations in settlement order, for bounded eviction.
    settled_order: Mutex<VecDeque<(RuntimeId, ClientMutationId)>>,
    /// Per-runtime settlement sinks. A sink is created lazily on first
    /// `emit_settlement` or `take_settlement_receiver` for a runtime.
    sinks: Mutex<HashMap<RuntimeId, SettlementSink>>,
}

impl RuntimeRegistry {
    pub(crate) fn new() -> Self {
        Self {
            mutations: Mutex::new(HashMap::new()),
            settled_order: Mutex::new(VecDeque::new()),
            sinks: Mutex::new(HashMap::new()),
        }
    }

    /// Atomically reserve a slot for `(runtime_id, client_mutation_id)` if
    /// absent. Returns `Existing(receipt)` if already known (the dedup case) or
    /// `New { runtime_mutation_id }` with a reserved pending entry. The lock is
    /// released before returning so the caller's `await` never holds it.
    pub(crate) fn accept(
        &self,
        runtime_id: &RuntimeId,
        client_mutation_id: &ClientMutationId,
        name: &str,
    ) -> ForwardAcceptance {
        let mut map = self.mutations.lock().expect("runtime registry lock");
        let key = (runtime_id.clone(), client_mutation_id.clone());
        if let Some(stored) = map.get(&key) {
            return ForwardAcceptance::Existing(stored.receipt());
        }
        let runtime_mutation_id =
            RuntimeMutationId::new(uuid::Uuid::new_v4().to_string());
        map.insert(
            key,
            StoredForwardMutation {
                runtime_mutation_id: runtime_mutation_id.clone(),
                client_mutation_id: client_mutation_id.clone(),
                name: name.to_string(),
                output: Value::Null,
                settled: false,
            },
        );
        ForwardAcceptance::New { runtime_mutation_id }
    }

    /// Fill a reserved entry's `output` once the mutation has applied.
    /// Idempotent: a missing entry (already evicted) is a no-op.
    pub(crate) fn settle_output(
        &self,
        runtime_id: &RuntimeId,
        client_mutation_id: &ClientMutationId,
        output: Value,
    ) {
        let mut map = self.mutations.lock().expect("runtime registry lock");
        if let Some(stored) = map.get_mut(&(runtime_id.clone(), client_mutation_id.clone())) {
            stored.output = output;
            stored.settled = true;
        }
        // A settled mutation is terminal → eligible for bounded eviction.
        let mut order = self.settled_order.lock().expect("settled-order lock");
        order.push_back((runtime_id.clone(), client_mutation_id.clone()));
        Self::prune_locked(&mut map, &mut order);
    }

    /// Drop the oldest terminal mutations once the retention window is full.
    /// Pending (never-settled) entries are never evicted.
    fn prune_locked(
        map: &mut HashMap<(RuntimeId, ClientMutationId), StoredForwardMutation>,
        order: &mut VecDeque<(RuntimeId, ClientMutationId)>,
    ) {
        while order.len() > MAX_RETAINED_MUTATIONS {
            let Some(key) = order.pop_front() else {
                break;
            };
            // Only evict terminal (settled) entries; a re-pushed key for a
            // still-pending mutation is left in place.
            if map.get(&key).map(|s| s.settled).unwrap_or(false) {
                map.remove(&key);
            }
        }
    }

    /// Route a `DownFrame::Settlement` onto the originating runtime's down-stream
    /// only (`settlement-routed-to-origin-runtime`) — never broadcast. The sink is
    /// created lazily so a Settlement emitted before the runtime subscribes is
    /// buffered for `take_settlement_receiver` to drain. `Base` frames are not sent
    /// here; they ride the backend's global event broadcast.
    pub(crate) fn emit_settlement(&self, runtime_id: &RuntimeId, frame: DownFrame) {
        let mut sinks = self.sinks.lock().expect("settlement-sink lock");
        let sink = sinks.entry(runtime_id.clone()).or_insert_with(SettlementSink::new);
        // Unbounded: the send cannot fail unless every receiver was dropped
        // (the runtime disconnected without resuming — S4); the frame is then
        // simply discarded, which is safe (the near node reconciles via `Base`).
        let _ = sink.tx.send(frame);
    }

    /// Get the originating runtime's settlement receiver for `subscribe_for` to
    /// merge with the `Base` broadcast. On a first subscription this is the
    /// receiver created with the channel; on a reconnect (the prior receiver was
    /// taken and dropped on disconnect) the channel is recreated so future
    /// Settlements drain to the fresh receiver (S4). Settlements buffered during
    /// the disconnect window are lost — best-effort, which is safe: the near node
    /// reconciles via `Base` absorption (Confirmed) + the up-channel error
    /// (Failed), so a missed Settlement never strands an outbox entry.
    pub(crate) fn subscribe_settlement(
        &self,
        runtime_id: &RuntimeId,
    ) -> mpsc::UnboundedReceiver<DownFrame> {
        let mut sinks = self.sinks.lock().expect("settlement-sink lock");
        let sink = sinks.entry(runtime_id.clone()).or_insert_with(SettlementSink::new);
        let rx = sink.rx.lock().expect("settlement-rx lock").take();
        match rx {
            Some(rx) => rx,
            None => {
                // Reconnect: recreate the channel. The prior sender (with any
                // buffered disconnect-window Settlements) is dropped.
                let (tx, rx) = mpsc::unbounded_channel();
                sink.tx = tx;
                rx
            }
        }
    }

    /// Drop a just-reserved entry whose mutation failed to apply (atomic apply),
    /// so a retry with the same `(RuntimeId, ClientMutationId)` re-accepts as `New`
    /// rather than resolving to a stale pending record. No `Settlement` is emitted
    /// — the near node learns of the failure from the up-channel error, and cannot
    /// match a `Settlement` it never received a receipt for.
    pub(crate) fn reject(&self, runtime_id: &RuntimeId, client_mutation_id: &ClientMutationId) {
        let mut map = self.mutations.lock().expect("runtime registry lock");
        map.remove(&(runtime_id.clone(), client_mutation_id.clone()));
    }
}

impl Default for RuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rid(s: &str) -> RuntimeId {
        RuntimeId(s.to_string())
    }
    fn cid(s: &str) -> ClientMutationId {
        ClientMutationId::new(s)
    }

    #[test]
    fn a_retried_mutation_dedups_to_the_same_receipt() {
        let registry = RuntimeRegistry::new();
        let r = rid("rt-A");
        let c = cid("op-1");
        let acceptance = registry.accept(&r, &c, "message.setKeywords");
        let runtime_mutation_id = match acceptance {
            ForwardAcceptance::New { runtime_mutation_id } => runtime_mutation_id,
            _ => panic!("first accept must be New"),
        };
        // The runtime applied the mutation and recorded its output.
        registry.settle_output(&r, &c, serde_json::json!({ "events": [] }));

        // A retry of the same (runtime, client) id resolves to the stored record.
        match registry.accept(&r, &c, "message.setKeywords") {
            ForwardAcceptance::Existing(receipt) => {
                assert_eq!(receipt.runtime_mutation_id, Some(runtime_mutation_id));
                assert_eq!(receipt.client_mutation_id, c);
            }
            _ => panic!("retry must dedup to Existing"),
        }
    }

    #[test]
    fn two_runtimes_may_independently_mint_the_same_client_mutation_id() {
        // `link-permits-fan-in`: idempotency is scoped per RuntimeId, so the same
        // ClientMutationId from two runtimes is two distinct mutations.
        let registry = RuntimeRegistry::new();
        let c = cid("op-1");
        let a = match registry.accept(&rid("rt-A"), &c, "message.destroy") {
            ForwardAcceptance::New { runtime_mutation_id } => runtime_mutation_id,
            _ => panic!("rt-A first accept must be New"),
        };
        let b = match registry.accept(&rid("rt-B"), &c, "message.destroy") {
            ForwardAcceptance::New { runtime_mutation_id } => runtime_mutation_id,
            _ => panic!("rt-B first accept must be New (distinct runtime)"),
        };
        assert_ne!(a, b, "distinct runtimes get distinct RuntimeMutationIds");
    }

    #[test]
    fn terminal_mutations_are_evicted_once_the_window_is_full() {
        // Fill past the retention cap; the oldest confirmed entries are dropped,
        // never a pending one.
        let registry = RuntimeRegistry::new();
        let r = rid("rt-A");
        for i in 0..(MAX_RETAINED_MUTATIONS + 5) {
            let c = cid(&format!("op-{i}"));
            registry.accept(&r, &c, "message.setKeywords");
            registry.settle_output(&r, &c, serde_json::json!({}));
        }
        let map = registry.mutations.lock().unwrap();
        assert!(map.len() <= MAX_RETAINED_MUTATIONS);
    }

    #[test]
    fn settlement_routes_only_to_the_originating_runtime() {
        use posthaste_link_contract::{WireMutationId, WireSettlementOutcome};
        let registry = RuntimeRegistry::new();
        let a = rid("rt-A");
        let b = rid("rt-B");
        registry.emit_settlement(
            &a,
            DownFrame::Settlement {
                mutation_id: WireMutationId("m-1".into()),
                outcome: WireSettlementOutcome::Confirmed,
            },
        );
        let mut rx_a = registry.subscribe_settlement(&a);
        let mut rx_b = registry.subscribe_settlement(&b);
        let frame = rx_a.try_recv().expect("rt-A receives its settlement");
        assert!(matches!(frame, DownFrame::Settlement { .. }));
        assert!(
            rx_b.try_recv().is_err(),
            "rt-B must not receive rt-A's settlement"
        );
    }

    #[test]
    fn a_reconnecting_runtime_resumes_its_settlement_stream() {
        use posthaste_link_contract::{WireMutationId, WireSettlementOutcome};
        let registry = RuntimeRegistry::new();
        let rt = rid("rt-A");
        // First subscription, then disconnect (drop the receiver).
        let first = registry.subscribe_settlement(&rt);
        drop(first);
        // Reconnect: a fresh subscription. A subsequent Settlement reaches it
        // (the channel was recreated) — S4.
        let mut second = registry.subscribe_settlement(&rt);
        registry.emit_settlement(
            &rt,
            DownFrame::Settlement {
                mutation_id: WireMutationId("m-2".into()),
                outcome: WireSettlementOutcome::Confirmed,
            },
        );
        let frame = second
            .try_recv()
            .expect("a reconnected runtime receives its settlement");
        assert!(matches!(frame, DownFrame::Settlement { .. }));
    }
}
