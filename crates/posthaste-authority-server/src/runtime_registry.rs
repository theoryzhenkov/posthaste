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

use std::time::{SystemTime, UNIX_EPOCH};

use posthaste_authority_server_link::{AuthorityServerFrame, AuthorityServerLinkId};
use posthaste_contract_core::{
    ClientMutationId, MutationReceipt, MutationSettlementState, RuntimeAdapterError,
    RuntimeMutationId,
};
use posthaste_link_far_end::{
    Accept, DedupStore, ReplayStore, Resume, Sequenced, SettlementSinkStore, TerminalClass,
};
use serde_json::Value;
use tokio::sync::mpsc;

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

/// The per-runtime registry: the runtime↔authority-server seam's assembly of the
/// shared far-end sub-stores.
pub(crate) struct RuntimeRegistry {
    dedup: DedupStore<AuthorityServerLinkId, StoredForwardMutation>,
    sinks: SettlementSinkStore<AuthorityServerLinkId, AuthorityServerFrame>,
    replay: ReplayStore<AuthorityServerLinkId, AuthorityServerFrame>,
}

impl RuntimeRegistry {
    pub(crate) fn new() -> Self {
        Self {
            // An AS link lives for a runtime's whole uptime (unlike a client
            // session), so its Rejected ledger is bounded (V14 follow-up knob):
            // oldest-first eviction at the default cap. The runtime seam's
            // assembly stays unbounded — a session's lifetime bounds it there.
            dedup: DedupStore::new()
                .with_rejected_capacity(posthaste_link_far_end::DEFAULT_REJECTED_CAPACITY),
            sinks: SettlementSinkStore::new(),
            replay: ReplayStore::new(),
        }
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
    /// `Confirmed`: kept, bounded-evicted).
    pub(crate) fn settle_confirmed(
        &self,
        runtime_id: &AuthorityServerLinkId,
        client_mutation_id: &ClientMutationId,
        output: Value,
    ) {
        self.dedup
            .settle(runtime_id, client_mutation_id, TerminalClass::Confirmed, |record| {
                record.output = output;
            });
    }

    /// Record a permanent rejection verdict (D47 `Rejected`: kept; a duplicate
    /// re-observes the same error, never re-executes).
    pub(crate) fn settle_rejected(
        &self,
        runtime_id: &AuthorityServerLinkId,
        client_mutation_id: &ClientMutationId,
        error: RuntimeAdapterError,
    ) {
        self.dedup
            .settle(runtime_id, client_mutation_id, TerminalClass::Rejected, |record| {
                record.error = Some(error);
            });
    }

    /// Clear a reserved entry whose apply failed transiently (D47 `Failed`:
    /// cleared; a deliberate retry re-accepts as `New` and re-executes).
    pub(crate) fn settle_failed(
        &self,
        runtime_id: &AuthorityServerLinkId,
        client_mutation_id: &ClientMutationId,
    ) {
        self.dedup
            .settle(runtime_id, client_mutation_id, TerminalClass::Failed, |_| {});
    }

    /// Route a `AuthorityServerFrame::Settlement` onto the originating runtime's
    /// sink only (`settlement-routed-to-origin-runtime`) — never broadcast. The
    /// down-stream stamps the seq when it drains the sink.
    pub(crate) fn emit_settlement(
        &self,
        runtime_id: &AuthorityServerLinkId,
        frame: AuthorityServerFrame,
    ) {
        self.sinks.emit(runtime_id, frame);
    }

    /// Take the originating runtime's settlement receiver for the down-stream to
    /// merge with the `Base` broadcast. Reconnect-safe (a fresh channel on
    /// resubscribe). Opportunistically reaps sinks whose subscriber has been gone
    /// past the TTL — the sink-leak fix — driven by the current wall-clock tick.
    pub(crate) fn subscribe_settlement(
        &self,
        runtime_id: &AuthorityServerLinkId,
    ) -> mpsc::UnboundedReceiver<AuthorityServerFrame> {
        let now = now_secs();
        self.sinks.reap(now);
        self.sinks.subscribe(runtime_id, now)
    }

    /// Resolve a (re)subscribe's resume point against the runtime's seq backlog
    /// (D46): fresh, replay-from-`after_seq`, or collapse-to-current-state.
    pub(crate) fn replay_resume(
        &self,
        runtime_id: &AuthorityServerLinkId,
        after_seq: Option<u64>,
    ) -> Resume<AuthorityServerFrame> {
        self.replay.resume(runtime_id, after_seq)
    }

    /// Stamp the next monotonic per-runtime seq onto a down-frame and retain it
    /// in the bounded backlog (D46).
    pub(crate) fn replay_record(
        &self,
        runtime_id: &AuthorityServerLinkId,
        frame: AuthorityServerFrame,
    ) -> Sequenced<AuthorityServerFrame> {
        self.replay.record(runtime_id, frame)
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
        registry.settle_confirmed(&r, &c, serde_json::json!({ "events": [] }));
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

    #[test]
    fn confirmed_mutations_are_evicted_once_the_window_is_full() {
        let registry = RuntimeRegistry::new();
        let r = rid("rt-A");
        for i in 0..(posthaste_link_far_end::DEFAULT_TERMINAL_CAPACITY + 5) {
            let c = cid(&format!("op-{i}"));
            registry.accept(&r, &c, "message.setKeywords");
            registry.settle_confirmed(&r, &c, serde_json::json!({}));
        }
        // The oldest Confirmed terminals fell out of the per-runtime window.
        assert!(matches!(
            registry.accept(&r, &cid("op-0"), "message.setKeywords"),
            ForwardAcceptance::New { .. }
        ));
    }

    // D47 at the seam: a permanent (non-retryable) rejection is KEPT — a retry
    // re-observes the same rejection and never re-executes.
    #[test]
    fn a_rejected_mutation_is_kept_and_re_observed_on_retry() {
        let registry = RuntimeRegistry::new();
        let (r, c) = (rid("rt-A"), cid("op-1"));
        registry.accept(&r, &c, "message.setKeywords");
        registry.settle_rejected(
            &r,
            &c,
            RuntimeAdapterError {
                code: RuntimeErrorCode::InvalidMutation,
                message: "nope".into(),
                retryable: false,
                correlation_id: None,
                details: Value::Null,
            },
        );
        match registry.accept(&r, &c, "message.setKeywords") {
            ForwardAcceptance::Rejected(error) => {
                assert_eq!(error.code, RuntimeErrorCode::InvalidMutation);
            }
            _ => panic!("a Rejected retry must re-observe the rejection"),
        }
    }

    // V14 follow-up knob (AS assembly): this seam's links outlive any client
    // session, so its Rejected ledger is BOUNDED — the oldest rejection falls
    // out of the window and a very late retry re-accepts as New instead of
    // re-observing a verdict held forever.
    #[test]
    fn the_rejected_window_is_bounded_at_this_seam() {
        let registry = RuntimeRegistry::new();
        let r = rid("rt-A");
        let rejection = |message: &str| RuntimeAdapterError {
            code: RuntimeErrorCode::InvalidMutation,
            message: message.into(),
            retryable: false,
            correlation_id: None,
            details: Value::Null,
        };
        for i in 0..(posthaste_link_far_end::DEFAULT_REJECTED_CAPACITY + 5) {
            let c = cid(&format!("rej-{i}"));
            registry.accept(&r, &c, "message.setKeywords");
            registry.settle_rejected(&r, &c, rejection("nope"));
        }
        // The oldest rejection was evicted: its retry re-accepts as New.
        assert!(matches!(
            registry.accept(&r, &cid("rej-0"), "message.setKeywords"),
            ForwardAcceptance::New { .. }
        ));
        // A recent rejection is still re-observed (the window holds the cap).
        let recent = format!("rej-{}", posthaste_link_far_end::DEFAULT_REJECTED_CAPACITY + 4);
        assert!(matches!(
            registry.accept(&r, &cid(&recent), "message.setKeywords"),
            ForwardAcceptance::Rejected(_)
        ));
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

    #[test]
    fn settlement_routes_only_to_the_originating_runtime() {
        let registry = RuntimeRegistry::new();
        let (a, b) = (rid("rt-A"), rid("rt-B"));
        registry.emit_settlement(
            &a,
            AuthorityServerFrame::Settlement {
                mutation_id: MutationId("m-1".into()),
                outcome: WireSettlementOutcome::Confirmed,
            },
        );
        let mut rx_a = registry.subscribe_settlement(&a);
        let mut rx_b = registry.subscribe_settlement(&b);
        assert!(matches!(
            rx_a.try_recv(),
            Ok(AuthorityServerFrame::Settlement { .. })
        ));
        assert!(rx_b.try_recv().is_err(), "rt-B must not receive rt-A's settlement");
    }

    #[test]
    fn a_reconnecting_runtime_resumes_its_settlement_stream() {
        let registry = RuntimeRegistry::new();
        let rt = rid("rt-A");
        let first = registry.subscribe_settlement(&rt);
        drop(first);
        let mut second = registry.subscribe_settlement(&rt);
        registry.emit_settlement(
            &rt,
            AuthorityServerFrame::Settlement {
                mutation_id: MutationId("m-2".into()),
                outcome: WireSettlementOutcome::Confirmed,
            },
        );
        assert!(matches!(
            second.try_recv(),
            Ok(AuthorityServerFrame::Settlement { .. })
        ));
    }

    // D46: the seq backlog stamps a monotonic per-runtime seq and resumes from
    // after_seq (or collapses when the resume point has been dropped).
    #[test]
    fn the_seq_backlog_stamps_and_resumes() {
        let registry = RuntimeRegistry::new();
        let rt = rid("rt-A");
        let s1 = registry.replay_record(&rt, AuthorityServerFrame::Heartbeat);
        let s2 = registry.replay_record(&rt, AuthorityServerFrame::Heartbeat);
        assert_eq!((s1.seq, s2.seq), (1, 2));
        assert!(matches!(registry.replay_resume(&rt, None), Resume::Fresh));
        match registry.replay_resume(&rt, Some(1)) {
            Resume::Replay(frames) => assert_eq!(frames.len(), 1),
            _ => panic!("resume within the backlog must replay"),
        }
    }
}
