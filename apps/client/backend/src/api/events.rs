//! `GET /events`: the one SSE broadcast.

use axum::extract::State;
use axum::response::sse::{Event as SseEvent, Sse};
use futures_util::Stream;
use posthaste_client_models::{DomainEventPayload, EventMessage};
use posthaste_domain_model::DomainEvent;
use tokio::sync::broadcast;

use super::{ApiState, COALESCE_WINDOW, HEARTBEAT_INTERVAL};

/// `GET /events`: the one SSE broadcast. Every message carries the current
/// store generation; most also carry a domain event. The first message is
/// the handshake and carries the run id, so a client detects a backend
/// restart (fresh run id = everything held is stale). A generation-only
/// heartbeat fills silences, and a lagged subscriber heals through a
/// generation-only message — payloads are prompts, never a ledger.
pub(crate) async fn handle_events(
    State(state): State<ApiState>,
) -> Sse<impl Stream<Item = Result<SseEvent, std::convert::Infallible>>> {
    let events = state.app.events;
    let mut receiver = events.subscribe();
    let stream = async_stream::stream! {
        yield Ok(sse_message(&EventMessage {
            generation: events.generation(),
            run_id: Some(events.run_id().to_string()),
            event: None,
        }));
        loop {
            let received = tokio::select! {
                () = tokio::time::sleep(HEARTBEAT_INTERVAL) => None,
                received = receiver.recv() => Some(received),
            };
            match received {
                // Silence: generation-only heartbeat.
                None => {
                    yield Ok(sse_message(&EventMessage {
                        generation: events.generation(),
                        run_id: None,
                        event: None,
                    }));
                }
                Some(Ok(first)) => {
                    // Coalesce the burst: gather everything arriving within
                    // the window, then flush one write batch stamped with
                    // the current generation.
                    let mut batch = vec![first];
                    let window = tokio::time::sleep(COALESCE_WINDOW);
                    tokio::pin!(window);
                    let mut closed = false;
                    loop {
                        tokio::select! {
                            () = &mut window => break,
                            received = receiver.recv() => match received {
                                Ok(event) => batch.push(event),
                                Err(broadcast::error::RecvError::Lagged(_)) => break,
                                Err(broadcast::error::RecvError::Closed) => {
                                    closed = true;
                                    break;
                                }
                            }
                        }
                    }
                    let generation = events.generation();
                    for event in batch {
                        yield Ok(sse_message(&EventMessage {
                            generation,
                            run_id: None,
                            event: Some(event_payload(event)),
                        }));
                    }
                    if closed {
                        break;
                    }
                }
                // Lagged: dropped payloads heal through the level-triggered
                // generation; the client refetches what it needs.
                Some(Err(broadcast::error::RecvError::Lagged(_))) => {
                    yield Ok(sse_message(&EventMessage {
                        generation: events.generation(),
                        run_id: None,
                        event: None,
                    }));
                }
                Some(Err(broadcast::error::RecvError::Closed)) => break,
            }
        }
    };
    Sse::new(stream)
}

fn sse_message(message: &EventMessage) -> SseEvent {
    SseEvent::default().data(serde_json::to_string(message).unwrap_or_default())
}

/// Map a domain event onto the wire payload: the topic plus scope ids, with
/// the kind-specific payload passed through verbatim.
fn event_payload(event: DomainEvent) -> DomainEventPayload {
    DomainEventPayload {
        kind: event.topic,
        account_id: event.account_id,
        message_id: event.message_id,
        mailbox_id: event.mailbox_id,
        payload: match event.payload {
            serde_json::Value::Null => None,
            payload => Some(payload),
        },
    }
}
