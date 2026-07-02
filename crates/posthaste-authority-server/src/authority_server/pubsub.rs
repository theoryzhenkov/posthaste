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
    /// gets a fresh receiver (S4).
    pub(crate) fn subscribe_settlement(
        &self,
        runtime_id: &AuthorityServerLinkId,
    ) -> mpsc::UnboundedReceiver<AuthorityServerFrame> {
        self.runtimes.subscribe_settlement(runtime_id)
    }
}
