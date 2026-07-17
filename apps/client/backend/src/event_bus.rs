//! The in-process event bus and the store generation.
//!
//! Every committed write publishes its domain events here and advances the
//! store generation — an atomic counter scoped to one backend run, paired
//! with a run id minted at startup. The API layer subscribes to broadcast
//! the stream to clients; query answers stamp the generation observed before
//! evaluation so staleness always resolves toward a refetch.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use posthaste_domain_model::DomainEvent;
use tokio::sync::broadcast;

/// Default capacity of the broadcast channel. Payloads are prompts, not a
/// ledger: a lagging subscriber loses payloads but keeps liveness, so a
/// bounded buffer is safe.
pub const DEFAULT_EVENT_CAPACITY: usize = 1024;

/// One broadcast bus of domain events plus the monotonic store generation.
///
/// Cheap to clone: all fields are shared handles. `publish` bumps the
/// generation once per non-empty batch and fans the events out to every
/// subscriber; `bump` advances the generation for a committed write that
/// produced no events of its own.
#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<DomainEvent>,
    generation: Arc<AtomicU64>,
    run_id: Arc<str>,
}

impl EventBus {
    /// Create a bus with the given channel capacity and a fresh random run id.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender,
            generation: Arc::new(AtomicU64::new(0)),
            run_id: uuid::Uuid::new_v4().to_string().into(),
        }
    }

    /// Subscribe to the domain-event stream. The API layer's SSE adapter and
    /// automation consumers each hold one receiver.
    pub fn subscribe(&self) -> broadcast::Receiver<DomainEvent> {
        self.sender.subscribe()
    }

    /// Publish a batch of committed-write events: bump the generation once
    /// (when the batch is non-empty) and send each event to all subscribers.
    /// Send errors (no subscribers) are ignored — the generation bump is the
    /// durable signal; the payloads are prompts.
    pub fn publish(&self, events: &[DomainEvent]) {
        if events.is_empty() {
            return;
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        for event in events {
            let _ = self.sender.send(event.clone());
        }
    }

    /// Advance the generation for a committed write with no events to
    /// broadcast, returning the new value.
    pub fn bump(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// The current store generation. Monotonic within one backend run.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// The run id minted at startup. A fresh run id tells clients to treat
    /// everything they hold as stale.
    pub fn run_id(&self) -> &str {
        &self.run_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_domain_model::AccountId;

    fn event() -> DomainEvent {
        DomainEvent {
            seq: 1,
            account_id: AccountId::from("a1"),
            topic: "message.updated".to_string(),
            occurred_at: "2026-01-01T00:00:00Z".to_string(),
            mailbox_id: None,
            message_id: None,
            payload: serde_json::json!({}),
        }
    }

    #[test]
    fn publish_bumps_generation_once_per_batch_and_fans_out() {
        let bus = EventBus::new(8);
        let mut receiver = bus.subscribe();
        assert_eq!(bus.generation(), 0);

        bus.publish(&[event(), event()]);
        assert_eq!(bus.generation(), 1, "one bump per batch, not per event");
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_ok());

        bus.publish(&[]);
        assert_eq!(bus.generation(), 1, "an empty batch is not a write");

        assert_eq!(bus.bump(), 2);
        assert_eq!(bus.generation(), 2);
    }

    #[test]
    fn clones_share_generation_and_run_id() {
        let bus = EventBus::new(8);
        let clone = bus.clone();
        bus.bump();
        assert_eq!(clone.generation(), 1);
        assert_eq!(bus.run_id(), clone.run_id());
    }
}
