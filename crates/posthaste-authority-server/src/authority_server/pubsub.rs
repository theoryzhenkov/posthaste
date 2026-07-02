//! The authority server's pub-sub surface (D29 split of
//! `authority_server.rs`): the authoritative domain-event broadcast and the
//! per-runtime settlement subscription the link's down-channel is built from.
//! Verbatim moves from `authority_server.rs`.
use super::*;

impl AuthorityServer {
    /// Publish authoritative domain events on the down-channel broadcast. In the
    /// co-located deployment this is the same event bus the runtime's views and
    /// the SSE event stream already consume.
    pub(crate) fn publish_events(&self, events: &[DomainEvent]) {
        for event in events {
            let _ = self.event_sender.send(event.clone());
        }
    }

    /// A receiver on the authoritative domain-event broadcast — the raw signal
    /// the link's down-channel is built from
    /// ([`LocalAuthorityServer`](crate::local_authority_server::LocalAuthorityServer)).
    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<DomainEvent> {
        self.event_sender.subscribe()
    }

    /// Get the originating runtime's settlement receiver (for `subscribe_for` to
    /// merge with this `Base` broadcast). Reconnect-safe: a reconnecting runtime
    /// gets a fresh receiver (S4); stale sinks are reaped (the sink-leak fix).
    pub(crate) fn subscribe_settlement(
        &self,
        runtime_id: &AuthorityServerLinkId,
    ) -> mpsc::UnboundedReceiver<AuthorityServerFrame> {
        self.runtimes.subscribe_settlement(runtime_id)
    }

    /// Resolve a (re)subscribe's resume point against the runtime's seq backlog
    /// (D46): fresh, replay-from-`after_seq`, or collapse-to-current-state.
    pub(crate) fn replay_resume(
        &self,
        runtime_id: &AuthorityServerLinkId,
        after_seq: Option<u64>,
    ) -> posthaste_link_far_end::Resume<AuthorityServerFrame> {
        self.runtimes.replay_resume(runtime_id, after_seq)
    }

    /// Stamp the next monotonic per-runtime seq onto a down-frame (D46), retain
    /// it in the bounded backlog, and return the wire-ready sequenced frame.
    pub(crate) fn replay_record(
        &self,
        runtime_id: &AuthorityServerLinkId,
        frame: AuthorityServerFrame,
    ) -> posthaste_link_far_end::Sequenced<AuthorityServerFrame> {
        self.runtimes.replay_record(runtime_id, frame)
    }
}
