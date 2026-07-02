//! The authority server's pub-sub surface (D29 split of
//! `authority_server.rs`): the authoritative domain-event broadcast and the
//! per-runtime link down-channel plumbing.
use super::*;
use crate::local_authority_server::base_frame_from_event;
use crate::runtime_registry::DownStreamChannels;

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
    ) -> posthaste_link_far_end::Resume<AuthorityServerFrame> {
        self.runtimes.replay_resume(runtime_id, after_seq)
    }

    /// The current resume cursor (highest issued seq) for a runtime — the value a
    /// `Reset` carries on a collapse (D49).
    pub(crate) fn highest_seq(&self, runtime_id: &AuthorityServerLinkId) -> u64 {
        self.runtimes.highest_seq(runtime_id)
    }
}
