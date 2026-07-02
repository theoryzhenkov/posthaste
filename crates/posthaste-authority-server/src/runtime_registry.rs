//! The authority server far node's per-runtime registry
//! ([replication authority-server-link L1 §3.1](../replication/authority-server-link/L1.md)):
//! the runtime↔authority-server seam's assembly of the shared far-end
//! sub-stores ([`posthaste_link_far_end`], RFC D40/D45).
//!
//! This side dedups the runtimes' forwarded mutations — mirroring the runtime
//! near node's own dedup one level up (a near node dedups its clients'
//! mutations; the authority server dedups the runtimes'). It composes three
//! sub-stores, all keyed per [`AuthorityServerLinkId`]:
//!
//! - [`DedupStore`] — `(AuthorityServerLinkId, ClientMutationId)` idempotency
//!   with the D47 terminal-class rule (Rejected kept, Failed cleared). `accept`
//!   atomically reserves a slot so a concurrent retry cannot double-apply.
//! - [`SettlementSinkStore`] — per-runtime settlement-to-originator routing
//!   (`settlement-routed-to-origin-runtime`) with a TTL reaper (the sink-leak
//!   fix this seam lacked).
//! - [`ReplayStore`] — the seq-backlog: a monotonic per-runtime seq stamped onto
//!   every down-frame, a bounded backlog, and resume-from-`after_seq` with the
//!   collapse fallback (D46 — replay this seam previously had none of).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use posthaste_authority_server_link::{AuthorityServerFrame, AuthorityServerLinkId, SequencedFrame};
use posthaste_contract_core::{
    ClientMutationId, MutationReceipt, MutationSettlementState, RuntimeAdapterError,
    RuntimeMutationId,
};
use posthaste_link_far_end::{
    Accept, DedupStore, ReplayStore, Resume, SettlementSinkStore, TerminalClass,
};
use serde_json::Value;
use tokio::sync::{broadcast, mpsc};

/// Capacity of a per-runtime **live** base broadcast. Base frames are recorded
/// into the replay backlog *before* this lossy hop (D49 [0] — record-at-emission),
/// so a lag here is fully recoverable: the near node's resubscribe replays the
/// gap from the complete backlog (or collapses to a `Reset`).
const BASE_LIVE_CAPACITY: usize = 512;

/// A forwarded mutation the authority server has accepted, stored for
/// `(AuthorityServerLinkId, ClientMutationId)` idempotency in the shared
/// [`DedupStore`].
#[derive(Clone)]
pub(crate) struct StoredForwardMutation {
    runtime_mutation_id: RuntimeMutationId,
    client_mutation_id: ClientMutationId,
    name: String,
    /// Serialized `CommandAck` — the receipt's `output`. `Null` while a just
    /// reserved entry is still applying.
    output: Value,
    /// D47: the permanent-rejection verdict for a kept `Rejected` settlement, so
    /// a duplicate `ClientMutationId` re-observes the same rejection instead of
    /// re-executing. `None` for pending / `Confirmed`.
    error: Option<RuntimeAdapterError>,
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

/// The outcome of reserving a mutation at the authority server up-channel.
pub(crate) enum ForwardAcceptance {
    /// First time this `(AuthorityServerLinkId, ClientMutationId)` was seen: the
    /// authority server assigned `RuntimeMutationId` and reserved a pending entry
    /// the caller must settle.
    New { runtime_mutation_id: RuntimeMutationId },
    /// Already accepted (pending or `Confirmed`): return the stored receipt
    /// (idempotent — never apply the user intent twice).
    Existing(MutationReceipt),
    /// D47: a kept permanent rejection. The caller returns this same error and
    /// does NOT re-execute.
    Rejected(RuntimeAdapterError),
}

/// Wall-clock seconds — the `now` tick the sink reaper is driven on.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A registered live down-stream for a runtime: its base broadcast sender + the
/// generation it was opened at (D49 [8]).
struct DownStreamHandle {
    base: broadcast::Sender<SequencedFrame>,
    generation: u64,
}

/// A live down-stream's receivers + the generation stamp it must check to detect
/// being superseded (D49 [8]).
pub(crate) struct DownStreamChannels {
    pub base: broadcast::Receiver<SequencedFrame>,
    pub settlement: mpsc::UnboundedReceiver<SequencedFrame>,
    pub generation: u64,
}

/// The per-runtime registry: the runtime↔authority-server seam's assembly of the
/// shared far-end sub-stores.
pub(crate) struct RuntimeRegistry {
    dedup: DedupStore<AuthorityServerLinkId, StoredForwardMutation>,
    /// Per-runtime settlement routing — the reaper here is the **departure**
    /// signal (D49 [6]/[9]): a link reaped for age purges all its per-link state.
    /// Carries pre-sequenced settlement frames (recorded at emit, D49 [0]).
    sinks: SettlementSinkStore<AuthorityServerLinkId, SequencedFrame>,
    /// The seq-backlog (D46): base + settlement frames recorded at emission, so
    /// the backlog is complete by construction (D49 [0]).
    replay: ReplayStore<AuthorityServerLinkId, AuthorityServerFrame>,
    /// Active down-streams: the base broadcast a base frame is recorded onto +
    /// the current generation (D49 [8]). A runtime with no entry here is not
    /// subscribed, so base frames are not recorded for it (it starts fresh).
    down_streams: Mutex<HashMap<AuthorityServerLinkId, DownStreamHandle>>,
}

impl RuntimeRegistry {
    pub(crate) fn new() -> Self {
        Self {
            // D48: uniform time-and-acknowledgment retention (no per-class count
            // windows, no Rejected-cap knob). The safety-valve cap + TTL + acked
            // cursor bound the ledger.
            dedup: DedupStore::new(),
            sinks: SettlementSinkStore::new(),
            replay: ReplayStore::new(),
            down_streams: Mutex::new(HashMap::new()),
        }
    }

    fn lock_down_streams(&self) -> std::sync::MutexGuard<'_, HashMap<AuthorityServerLinkId, DownStreamHandle>> {
        self.down_streams
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Atomically reserve a slot for `(runtime_id, client_mutation_id)`. Returns
    /// the D47 verdict: `New` (reserved a pending entry), `Existing` (dedup to a
    /// pending/Confirmed receipt), or `Rejected` (re-observe a kept permanent
    /// verdict). The lock is released before returning so the caller's `await`
    /// never holds it.
    pub(crate) fn accept(
        &self,
        runtime_id: &AuthorityServerLinkId,
        client_mutation_id: &ClientMutationId,
        name: &str,
    ) -> ForwardAcceptance {
        let runtime_mutation_id = RuntimeMutationId::new(uuid::Uuid::new_v4().to_string());
        let record = StoredForwardMutation {
            runtime_mutation_id: runtime_mutation_id.clone(),
            client_mutation_id: client_mutation_id.clone(),
            name: name.to_string(),
            output: Value::Null,
            error: None,
        };
        match self.dedup.accept(runtime_id, client_mutation_id, || record) {
            Accept::New => ForwardAcceptance::New { runtime_mutation_id },
            Accept::Duplicate(stored) => match stored.error {
                Some(error) => ForwardAcceptance::Rejected(error),
                None => ForwardAcceptance::Existing(stored.receipt()),
            },
        }
    }

    /// Fill a reserved entry's `output` once the mutation has applied (D47
    /// `Confirmed`: kept — D48 retention). `settlement_seq` is the replay seq of
    /// the emitted Settlement frame (the ack target).
    pub(crate) fn settle_confirmed(
        &self,
        runtime_id: &AuthorityServerLinkId,
        client_mutation_id: &ClientMutationId,
        output: Value,
        settlement_seq: u64,
    ) {
        self.dedup.settle(
            runtime_id,
            client_mutation_id,
            TerminalClass::Confirmed,
            Some(settlement_seq),
            now_secs(),
            |record| record.output = output,
        );
    }

    /// Record a permanent rejection verdict (D47 `Rejected`: kept; a duplicate
    /// re-observes the same error, never re-executes). This seam emits **no**
    /// settlement frame for a rejection (the near node learns of it via the
    /// up-channel error), so there is no ack seq — TTL/cap govern its retention.
    pub(crate) fn settle_rejected(
        &self,
        runtime_id: &AuthorityServerLinkId,
        client_mutation_id: &ClientMutationId,
        error: RuntimeAdapterError,
    ) {
        self.dedup.settle(
            runtime_id,
            client_mutation_id,
            TerminalClass::Rejected,
            None,
            now_secs(),
            |record| record.error = Some(error),
        );
    }

    /// Clear a reserved entry whose apply failed transiently (D47 `Failed`:
    /// cleared; a deliberate retry re-accepts as `New` and re-executes).
    pub(crate) fn settle_failed(
        &self,
        runtime_id: &AuthorityServerLinkId,
        client_mutation_id: &ClientMutationId,
    ) {
        self.dedup.settle(
            runtime_id,
            client_mutation_id,
            TerminalClass::Failed,
            None,
            0,
            |_| {},
        );
    }

    /// Record a `Settlement` frame at **emission** into the originating runtime's
    /// backlog (D49 [0]) and route it onto that runtime's sink only
    /// (`settlement-routed-to-origin-runtime`) — never broadcast. Returns the
    /// replay seq stamped onto it, which the caller feeds to [`settle_confirmed`]
    /// as the ack target (D48).
    pub(crate) fn emit_settlement(
        &self,
        runtime_id: &AuthorityServerLinkId,
        frame: AuthorityServerFrame,
    ) -> u64 {
        let stamped = self.replay.record(runtime_id, frame);
        let seq = stamped.seq();
        self.sinks.emit(runtime_id, stamped, now_secs());
        seq
    }

    /// Record a `Base` frame at **emission** (D49 [0]) into every currently
    /// subscribed runtime's backlog — before the lossy live broadcast — so the
    /// backlog is complete by construction, then deliver it live. A runtime with
    /// no live down-stream is skipped (it holds no backlog until it subscribes).
    pub(crate) fn record_base(&self, frame: AuthorityServerFrame) {
        let runtimes: Vec<AuthorityServerLinkId> =
            self.lock_down_streams().keys().cloned().collect();
        for runtime_id in runtimes {
            let stamped = self.replay.record(&runtime_id, frame.clone());
            if let Some(handle) = self.lock_down_streams().get(&runtime_id) {
                let _ = handle.base.send(stamped);
            }
        }
    }

    /// Register a live down-stream for a runtime and hand back its channels + the
    /// generation it opened at (D49 [8]). A fresh generation supersedes any prior
    /// down-stream for the runtime — the older one observes the mismatch and
    /// terminates. Opportunistically drives the departure reaper (D49 [6]/[9]).
    pub(crate) fn register_down_stream(
        &self,
        runtime_id: &AuthorityServerLinkId,
    ) -> DownStreamChannels {
        let now = now_secs();
        self.reap(now);
        let settlement = self.sinks.subscribe(runtime_id, now);
        let mut streams = self.lock_down_streams();
        let entry = streams
            .entry(runtime_id.clone())
            .or_insert_with(|| DownStreamHandle {
                base: broadcast::channel(BASE_LIVE_CAPACITY).0,
                generation: 0,
            });
        entry.generation += 1;
        DownStreamChannels {
            base: entry.base.subscribe(),
            settlement,
            generation: entry.generation,
        }
    }

    /// The current generation for a runtime — a down-stream whose stamp no longer
    /// matches has been superseded and must terminate (D49 [8]).
    pub(crate) fn current_generation(&self, runtime_id: &AuthorityServerLinkId) -> u64 {
        self.lock_down_streams()
            .get(runtime_id)
            .map(|h| h.generation)
            .unwrap_or(0)
    }

    /// Resolve a (re)subscribe's resume point against the runtime's seq backlog
    /// (D46): fresh, replay-from-`after_seq`, or collapse-to-current-state. The
    /// resume cursor IS the ack signal (D48): a resume from `after_seq` means the
    /// runtime has seen every frame up to it, so terminal dedup records whose
    /// settlement frame it has passed are reclaimed.
    pub(crate) fn replay_resume(
        &self,
        runtime_id: &AuthorityServerLinkId,
        after_seq: Option<u64>,
    ) -> Resume<AuthorityServerFrame> {
        if let Some(cursor) = after_seq {
            self.dedup.ack(runtime_id, cursor);
        }
        self.replay.resume(runtime_id, after_seq)
    }

    /// The current resume cursor (highest issued seq) for a runtime — the value a
    /// `Reset` carries when a resume collapses (D49).
    pub(crate) fn highest_seq(&self, runtime_id: &AuthorityServerLinkId) -> u64 {
        self.replay.highest_seq(runtime_id)
    }

    /// The departure reaper (D49 [6]/[9]): reap settlement sinks that have aged
    /// out (subscriber gone past the TTL, or a never-subscribed sink stale by
    /// age) and, for each reaped runtime, purge ALL its per-link state — dedup,
    /// replay backlog, and the live down-stream registration. Also runs the
    /// dedup TTL fallback (D48 (b)).
    pub(crate) fn reap(&self, now: u64) {
        for departed in self.sinks.reap(now) {
            self.dedup.purge(&departed);
            self.replay.purge(&departed);
            self.lock_down_streams().remove(&departed);
        }
        self.dedup.reap(now);
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
    use posthaste_authority_server_link::WireSettlementOutcome;
    use posthaste_contract_core::RuntimeErrorCode;
    use posthaste_link_core::MutationId;

    fn rid(s: &str) -> AuthorityServerLinkId {
        AuthorityServerLinkId(s.to_string())
    }
    fn cid(s: &str) -> ClientMutationId {
        ClientMutationId::new(s)
    }

    #[test]
    fn a_retried_mutation_dedups_to_the_same_receipt() {
        let registry = RuntimeRegistry::new();
        let (r, c) = (rid("rt-A"), cid("op-1"));
        let runtime_mutation_id = match registry.accept(&r, &c, "message.setKeywords") {
            ForwardAcceptance::New { runtime_mutation_id } => runtime_mutation_id,
            _ => panic!("first accept must be New"),
        };
        registry.settle_confirmed(&r, &c, serde_json::json!({ "events": [] }), 1);
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

    fn rejection() -> RuntimeAdapterError {
        RuntimeAdapterError {
            code: RuntimeErrorCode::InvalidMutation,
            message: "nope".into(),
            retryable: false,
            correlation_id: None,
            details: Value::Null,
        }
    }

    // D47 at the seam: a permanent (non-retryable) rejection is KEPT — a retry
    // re-observes the same rejection and never re-executes.
    #[test]
    fn a_rejected_mutation_is_kept_and_re_observed_on_retry() {
        let registry = RuntimeRegistry::new();
        let (r, c) = (rid("rt-A"), cid("op-1"));
        registry.accept(&r, &c, "message.setKeywords");
        registry.settle_rejected(&r, &c, rejection());
        match registry.accept(&r, &c, "message.setKeywords") {
            ForwardAcceptance::Rejected(error) => {
                assert_eq!(error.code, RuntimeErrorCode::InvalidMutation);
            }
            _ => panic!("a Rejected retry must re-observe the rejection"),
        }
    }

    // D48 (uniform, this seam): a rejection has no settlement frame, so an ack
    // cannot reclaim it — it is re-observable until the TTL, not held forever
    // (the M9b2 Rejected-cap knob is gone). A Confirmed carries a settlement seq,
    // so an acked cursor past it reclaims the record.
    #[test]
    fn d48_confirmed_reclaims_on_ack_rejection_only_on_ttl() {
        let registry = RuntimeRegistry::new();
        let r = rid("rt-A");
        // Confirmed with settlement seq 3; a resume past 3 acks + reclaims it.
        let (cc, cr) = (cid("cf"), cid("rej"));
        registry.accept(&r, &cc, "message.setKeywords");
        registry.settle_confirmed(&r, &cc, serde_json::json!({}), 3);
        registry.accept(&r, &cr, "message.setKeywords");
        registry.settle_rejected(&r, &cr, rejection());
        // Resume past seq 3 acks the confirmed record (reclaimed → re-accepts New)
        // but cannot touch the frameless rejection (still re-observed).
        let _ = registry.replay_resume(&r, Some(5));
        assert!(
            matches!(registry.accept(&r, &cc, "message.setKeywords"), ForwardAcceptance::New { .. }),
            "an acked confirmed record is reclaimed"
        );
        assert!(
            matches!(registry.accept(&r, &cr, "message.setKeywords"), ForwardAcceptance::Rejected(_)),
            "a frameless rejection is not acked away — re-observed until TTL"
        );
    }

    // D47 at the seam: a transient (retryable) failure is CLEARED — a deliberate
    // retry re-accepts as New and re-executes.
    #[test]
    fn a_failed_mutation_is_cleared_and_re_executes_on_retry() {
        let registry = RuntimeRegistry::new();
        let (r, c) = (rid("rt-A"), cid("op-1"));
        registry.accept(&r, &c, "message.setKeywords");
        registry.settle_failed(&r, &c);
        assert!(matches!(
            registry.accept(&r, &c, "message.setKeywords"),
            ForwardAcceptance::New { .. }
        ));
    }

    fn confirmed(m: &str) -> AuthorityServerFrame {
        AuthorityServerFrame::Settlement {
            mutation_id: MutationId(m.into()),
            outcome: WireSettlementOutcome::Confirmed,
        }
    }

    #[test]
    fn settlement_routes_only_to_the_originating_runtime() {
        let registry = RuntimeRegistry::new();
        let (a, b) = (rid("rt-A"), rid("rt-B"));
        let mut ch_a = registry.register_down_stream(&a);
        let mut ch_b = registry.register_down_stream(&b);
        registry.emit_settlement(&a, confirmed("m-1"));
        assert!(
            matches!(ch_a.settlement.try_recv(), Ok(frame) if frame.frame().map(|f| matches!(f, AuthorityServerFrame::Settlement { .. })).unwrap_or(false)),
        );
        assert!(ch_b.settlement.try_recv().is_err(), "rt-B must not receive rt-A's settlement");
    }

    // D49 [8]: a new down-stream supersedes the prior one — the generation stamp
    // advances, and the old generation no longer matches the current.
    #[test]
    fn a_new_down_stream_supersedes_the_prior_generation() {
        let registry = RuntimeRegistry::new();
        let rt = rid("rt-A");
        let first = registry.register_down_stream(&rt);
        let second = registry.register_down_stream(&rt);
        assert_ne!(first.generation, second.generation);
        assert_eq!(registry.current_generation(&rt), second.generation);
        assert_ne!(registry.current_generation(&rt), first.generation, "the first is superseded");
    }

    // D49 [0]: a base frame is recorded at emission into every subscribed
    // runtime's backlog and delivered live; the backlog is complete (resumable).
    #[test]
    fn base_frames_record_at_emission_and_deliver_live() {
        let registry = RuntimeRegistry::new();
        let rt = rid("rt-A");
        let mut ch = registry.register_down_stream(&rt);
        registry.record_base(AuthorityServerFrame::Heartbeat);
        registry.record_base(AuthorityServerFrame::Heartbeat);
        // Delivered live on the base broadcast, stamped 1..2.
        assert_eq!(ch.base.try_recv().unwrap().seq(), 1);
        assert_eq!(ch.base.try_recv().unwrap().seq(), 2);
        // And retained in the backlog: a resume from 0 replays both.
        match registry.replay_resume(&rt, Some(0)) {
            Resume::Replay(frames) => assert_eq!(frames.len(), 2),
            _ => panic!("the backlog is complete"),
        }
        // A runtime with no down-stream registered records nothing.
        registry.record_base(AuthorityServerFrame::Heartbeat);
        assert!(matches!(registry.replay_resume(&rid("rt-B"), Some(0)), Resume::Fresh));
    }

    // D49 [6]: when the sink reaper reaps a departed runtime, ALL its per-link
    // state is purged — dedup, replay backlog, and the down-stream registration.
    #[test]
    fn departure_purges_all_per_link_state() {
        let registry = RuntimeRegistry::new();
        let rt = rid("rt-A");
        // A subscriber that connects then vanishes, with dedup + backlog state.
        let ch = registry.register_down_stream(&rt);
        registry.accept(&rt, &cid("op"), "message.setKeywords");
        registry.settle_rejected(&rt, &cid("op"), rejection());
        registry.record_base(AuthorityServerFrame::Heartbeat);
        drop(ch); // subscriber gone
        // Drive the reaper past the sink TTL: departure purges everything.
        let ttl = posthaste_link_far_end::DEFAULT_SINK_TTL;
        registry.reap(1); // starts the countdown
        registry.reap(ttl + 3); // past TTL → reaped + purged
        assert!(
            matches!(registry.accept(&rt, &cid("op"), "message.setKeywords"), ForwardAcceptance::New { .. }),
            "dedup purged on departure"
        );
        assert_eq!(registry.highest_seq(&rt), 0, "replay backlog purged on departure");
        assert_eq!(registry.current_generation(&rt), 0, "down-stream registration purged");
    }
}
