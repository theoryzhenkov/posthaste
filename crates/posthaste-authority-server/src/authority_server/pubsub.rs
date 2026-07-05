//! The authority server's pub-sub surface (D29 split of
//! `authority_server.rs`): the authoritative domain-event broadcast and the
//! per-runtime link down-channel plumbing.
use super::*;
use crate::local_authority_server::base_frame_from_event;
use crate::runtime_registry::DownStreamChannels;
use posthaste_contract_core::{RuntimeAdapterError, RuntimeErrorCode, Terminality};
use posthaste_domain_model::{
    OperationDispatchUncertain, OperationOutcome, OperationSettlement,
    EVENT_TOPIC_OPERATION_DISPATCH_UNCERTAIN, EVENT_TOPIC_OPERATION_SETTLED,
};
use std::sync::Arc;
use tokio::sync::broadcast;

impl AuthorityServer {
    /// Publish authoritative domain events. Two consumers:
    ///
    /// 1. the shared event bus (the runtime's views + the SSE event stream);
    /// 2. the link's per-runtime down-channel — the derived `Base` frame is
    ///    recorded into every subscribed runtime's replay backlog **at emission**,
    ///    before the lossy live broadcast (D49 [0]: record-at-emission → the
    ///    backlog is complete by construction, so a lagged live delivery is fully
    ///    recoverable by a resubscribe/replay rather than silently lost).
    pub(crate) fn publish_events(&self, events: &[DomainEvent]) {
        for event in events {
            let _ = self.event_sender.send(event.clone());
            if let Some(frame) = base_frame_from_event(self, event) {
                self.runtimes.record_base(frame);
            }
        }
    }

    /// Register a live link down-stream for a runtime: its base broadcast + routed
    /// settlement receiver + the generation stamp (D49 [8]). Reconnect-safe and
    /// self-superseding — a new down-stream terminates the prior one.
    pub(crate) fn register_down_stream(
        &self,
        runtime_id: &AuthorityServerLinkId,
    ) -> DownStreamChannels {
        self.runtimes.register_down_stream(runtime_id)
    }

    /// The send-bridge terminal translation (step 3): map an async outbox
    /// settlement DomainEvent to a routed `Settlement` frame for the originating
    /// runtime, ALONGSIDE (never replacing) the DomainEvent other consumers rely
    /// on (views / the near-end reconciler / SSE). A no-op for any event that is
    /// not a DEFERRED async op's terminal settlement — a settlement with no
    /// registered send-origin was already given its verdict at enqueue (every
    /// non-Send op) and needs no routed frame here.
    ///
    /// Terminal-class mapping (D125/D126):
    /// - `operation.settled` `Applied` → `Settlement{Confirmed}` — the send left
    ///   Drafts; the draft-Destroy fold stays absorbed, the Sent row syncs in.
    /// - `operation.settled` `Failed`  → `Settlement{Failed}` — the client reverts
    ///   the fold (the draft RETURNS) and surfaces the error.
    /// - `operation.dispatch_uncertain` → `Settlement{Failed}` — a parked send:
    ///   the draft RETURNS + the M32 park surface, and NO false Sent (a parked
    ///   send is not a confirmed send).
    pub(crate) fn route_async_settlement(&self, event: &DomainEvent) {
        match event.topic.as_str() {
            EVENT_TOPIC_OPERATION_SETTLED => {
                let Ok(settlement) =
                    serde_json::from_value::<OperationSettlement>(event.payload.clone())
                else {
                    return;
                };
                let Some(origin) = self.runtimes.take_send_origin(&settlement.id) else {
                    return;
                };
                match settlement.outcome {
                    OperationOutcome::Applied => self
                        .runtimes
                        .settle_async_confirmed(&origin, serde_json::json!({ "events": [] })),
                    OperationOutcome::Failed => self.runtimes.settle_async_failed(
                        &origin,
                        send_bridge_error(
                            settlement
                                .error
                                .unwrap_or_else(|| "send failed".to_string()),
                        ),
                    ),
                }
            }
            EVENT_TOPIC_OPERATION_DISPATCH_UNCERTAIN => {
                let Ok(uncertain) =
                    serde_json::from_value::<OperationDispatchUncertain>(event.payload.clone())
                else {
                    return;
                };
                let Some(origin) = self.runtimes.take_send_origin(&uncertain.id) else {
                    return;
                };
                self.runtimes
                    .settle_async_failed(&origin, send_bridge_error(uncertain.reason));
            }
            _ => {}
        }
    }

    /// The current down-stream generation for a runtime (D49 [8]).
    pub(crate) fn current_generation(&self, runtime_id: &AuthorityServerLinkId) -> u64 {
        self.runtimes.current_generation(runtime_id)
    }

    /// Resolve a (re)subscribe's resume point against the runtime's seq backlog
    /// (D46) — also the ack signal that reclaims seen dedup verdicts (D48).
    pub(crate) fn replay_resume(
        &self,
        runtime_id: &AuthorityServerLinkId,
        after_seq: Option<u64>,
    ) -> posthaste_link_far_end::down::Resume<AuthorityServerFrame> {
        self.runtimes.replay_resume(runtime_id, after_seq)
    }

    /// The current resume cursor (highest issued seq) for a runtime — the value a
    /// `Reset` carries on a collapse (D49).
    pub(crate) fn highest_seq(&self, runtime_id: &AuthorityServerLinkId) -> u64 {
        self.runtimes.highest_seq(runtime_id)
    }
}

/// The carried error for a send-bridge `Failed`/parked verdict. `Permanent` (via
/// `Internal`) so the dedup record is KEPT — a duplicate forward of the same
/// mutation re-observes the rejection rather than re-executing the send.
fn send_bridge_error(message: String) -> RuntimeAdapterError {
    RuntimeAdapterError {
        code: RuntimeErrorCode::Internal,
        message,
        terminality: Terminality::Permanent,
        correlation_id: None,
        details: serde_json::Value::Null,
    }
}

/// Spawn the send-bridge (step 3): a bus subscriber that drives
/// [`AuthorityServer::route_async_settlement`] for every domain event, turning an
/// async op's terminal outbox settlement into a routed `Settlement` frame. The
/// task holds a `Weak` handle so it self-terminates when the authority server is
/// dropped; a broadcast lag skips (the missed frame's origin stays registered and
/// is reclaimed on departure/TTL). Must run within a Tokio runtime.
pub(crate) fn spawn_settlement_bridge(
    authority_server: &Arc<AuthorityServer>,
    event_sender: &broadcast::Sender<DomainEvent>,
) {
    let weak = Arc::downgrade(authority_server);
    let mut rx = event_sender.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let Some(authority_server) = weak.upgrade() else {
                        break;
                    };
                    authority_server.route_async_settlement(&event);
                }
                // Lag: skip the missed frame and keep looping (the match is the
                // loop's tail, so falling through re-iterates).
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}
