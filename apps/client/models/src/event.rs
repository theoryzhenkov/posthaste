//! The event stream: every SSE message carries the current store generation,
//! and most also carry a domain event. Payloads are prompts that trigger
//! reads — state comes from queries alone.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One message on `GET /events`. A generation-only message is a heartbeat;
/// the generation is level-triggered (every message states current state), so
/// a dropped message heals at the next one.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EventMessage {
    #[ts(type = "number")]
    pub generation: u64,
    /// The backend run id, carried by the stream's first message (the
    /// handshake). Generations are monotonic within one run; a fresh run id
    /// tells the client to treat everything it holds as stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub run_id: Option<String>,
    /// The domain event, when this message carries one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub event: Option<DomainEventPayload>,
}

/// A domain event as broadcast to clients: what happened, where, and a
/// kind-specific payload. Never folded into client state — it prompts a
/// refetch, a notification, a script.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DomainEventPayload {
    /// The event topic, e.g. `message.updated`, `sync.completed`,
    /// `operation.settled`.
    pub kind: String,
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<crate::mirror::MessageId>")]
    pub message_id: Option<domain::MessageId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<crate::mirror::MailboxId>")]
    pub mailbox_id: Option<domain::MailboxId>,
    /// Kind-specific payload, passed through verbatim; absent when the event
    /// carries no payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "unknown")]
    pub payload: Option<serde_json::Value>,
}
