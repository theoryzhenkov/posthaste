//! The authority server's own [`FactLog`] binding (RFC-L2-scripting D52 / S3).
//!
//! `posthaste-runtime`'s [`EventLogFactLog`] (S1/S2) is a **read-only** view of
//! the authority-authored `event_log`, reached through the runtime's
//! `ReadCache` — appends are rejected ([`FactLogError::ReadOnly`]) because the
//! runtime never authors facts, it only replays them. This binding is the other
//! half the runtime's doc comment names as "S3's": the **authoring** side,
//! reached directly off the authority server's own store, with a real
//! [`FactLog::append`].
//!
//! Investigation finding (S3): the bundled/in-process deployment does not need
//! a *second HTTP tap mount* — `/v1/events` (S2) already streams durably for
//! any fact that reaches `event_log`, because the runtime shares the authority
//! server's own `event_sender` broadcast and store in-process (one tap, per
//! D52's discipline). What WAS missing is a durable **authoring path** for
//! AS-originated meta-facts: the rule engine's `rule.fired` /
//! `rule.delivery.failed` facts were built by hand with `seq: 0` and sent
//! straight to the broadcast channel, bypassing `event_log` entirely — durable
//! for a live subscriber, but silently gone (no fact, no gap frame) for anyone
//! who reconnects. This binding closes that gap: the rule engine now appends
//! through the same durable, seq-assigning path `/v1/events` already replays
//! from, instead of a parallel ad hoc broadcast.
//!
//! @spec docs/eph/RFC-L2-scripting#4-d52-the-tap

use std::sync::Arc;

use async_trait::async_trait;
use posthaste_domain_model::{DomainEvent, EventFilter};
use posthaste_domain_service::MailStore;
use posthaste_link_far_end::down::{FactLog, FactLogError, Sequenced};
use tokio::sync::broadcast;

/// The authority server's writable fact log: `append` durably inserts into
/// `event_log` (assigning the real, monotonic seq) and then broadcasts the
/// persisted event on the same `event_sender` bus `/v1/events`'s live tail
/// forwards — so a durably-appended fact is visible to both a live subscriber
/// and a later durable replay, exactly like every other authority-server-
/// authored fact (message mutations, account lifecycle, sync completion).
pub(crate) struct AuthorityServerFactLog {
    store: Arc<dyn MailStore>,
    event_sender: broadcast::Sender<DomainEvent>,
}

impl AuthorityServerFactLog {
    pub(crate) fn new(store: Arc<dyn MailStore>, event_sender: broadcast::Sender<DomainEvent>) -> Self {
        Self { store, event_sender }
    }
}

#[async_trait]
impl FactLog for AuthorityServerFactLog {
    type Fact = DomainEvent;
    type Filter = EventFilter;

    /// Durably append `fact` (its `seq` is ignored — the store assigns the
    /// authoritative one), then broadcast the persisted event. The one write
    /// path AS-origin facts should use instead of hand-building a `seq: 0`
    /// event and sending it straight to the bus (the defect this closes).
    async fn append(&self, fact: DomainEvent) -> Result<u64, FactLogError> {
        let event = self
            .store
            .append_event(
                &fact.account_id,
                &fact.topic,
                fact.mailbox_id.as_ref(),
                fact.message_id.as_ref(),
                fact.payload,
            )
            .map_err(|error| FactLogError::Backing(error.to_string()))?;
        let seq = event.seq.max(0) as u64;
        let _ = self.event_sender.send(event);
        Ok(seq)
    }

    async fn replay(
        &self,
        after_seq: u64,
        filter: Option<EventFilter>,
    ) -> Result<Vec<Sequenced<DomainEvent>>, FactLogError> {
        let mut filter = filter.unwrap_or(EventFilter {
            account_id: None,
            topic: None,
            mailbox_id: None,
            after_seq: Some(after_seq as i64),
        });
        filter.after_seq = Some(after_seq as i64);
        let events = self
            .store
            .list_events(&filter)
            .map_err(|error| FactLogError::Backing(error.to_string()))?;
        Ok(events
            .into_iter()
            .map(|event| Sequenced::new(event.seq.max(0) as u64, event))
            .collect())
    }

    async fn highest_seq(&self) -> Result<u64, FactLogError> {
        match self
            .store
            .event_log_bounds()
            .map_err(|error| FactLogError::Backing(error.to_string()))?
        {
            Some(bounds) => Ok(bounds.newest.max(0) as u64),
            None => Ok(0),
        }
    }

    async fn truncation_point(&self) -> Result<u64, FactLogError> {
        match self
            .store
            .event_log_bounds()
            .map_err(|error| FactLogError::Backing(error.to_string()))?
        {
            Some(bounds) => Ok(bounds.oldest.max(0) as u64),
            None => Ok(0),
        }
    }
}
