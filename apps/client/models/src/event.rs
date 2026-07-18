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

/// Every event topic the server emits — the closed `kind` vocabulary of
/// [`DomainEventPayload`], one variant per
/// [`posthaste_domain_model::ALL_EVENT_TOPICS`] entry (drift-checked by
/// `tests/event_kind_drift.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum DomainEventKind {
    #[serde(rename = "sync.completed")]
    SyncCompleted,
    #[serde(rename = "sync.failed")]
    SyncFailed,
    #[serde(rename = "settings.updated")]
    SettingsUpdated,
    #[serde(rename = "config.reloaded")]
    ConfigReloaded,
    #[serde(rename = "smart_mailbox.created")]
    SmartMailboxCreated,
    #[serde(rename = "smart_mailbox.updated")]
    SmartMailboxUpdated,
    #[serde(rename = "smart_mailbox.deleted")]
    SmartMailboxDeleted,
    #[serde(rename = "smart_mailbox.reset")]
    SmartMailboxReset,
    #[serde(rename = "message.updated")]
    MessageUpdated,
    #[serde(rename = "message.body_cached")]
    MessageBodyCached,
    #[serde(rename = "mailbox.updated")]
    MailboxUpdated,
    #[serde(rename = "account.updated")]
    AccountUpdated,
    #[serde(rename = "account.created")]
    AccountCreated,
    #[serde(rename = "account.deleted")]
    AccountDeleted,
    #[serde(rename = "account.status_changed")]
    AccountStatusChanged,
    #[serde(rename = "push.connected")]
    PushConnected,
    #[serde(rename = "push.disconnected")]
    PushDisconnected,
    #[serde(rename = "operation.settled")]
    OperationSettled,
    #[serde(rename = "operation.dispatch_uncertain")]
    OperationDispatchUncertain,
    #[serde(rename = "rule.fired")]
    RuleFired,
    #[serde(rename = "rule.delivery.failed")]
    RuleDeliveryFailed,
}

impl DomainEventKind {
    /// Every variant, in [`posthaste_domain_model::ALL_EVENT_TOPICS`] order
    /// (asserted by the drift test).
    pub const ALL: &'static [DomainEventKind] = &[
        Self::SyncCompleted,
        Self::SyncFailed,
        Self::SettingsUpdated,
        Self::ConfigReloaded,
        Self::SmartMailboxCreated,
        Self::SmartMailboxUpdated,
        Self::SmartMailboxDeleted,
        Self::SmartMailboxReset,
        Self::MessageUpdated,
        Self::MessageBodyCached,
        Self::MailboxUpdated,
        Self::AccountUpdated,
        Self::AccountCreated,
        Self::AccountDeleted,
        Self::AccountStatusChanged,
        Self::PushConnected,
        Self::PushDisconnected,
        Self::OperationSettled,
        Self::OperationDispatchUncertain,
        Self::RuleFired,
        Self::RuleDeliveryFailed,
    ];
}

/// A domain event as broadcast to clients: what happened, where, and a
/// kind-specific payload. Never folded into client state — it prompts a
/// refetch, a notification, a script.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DomainEventPayload {
    /// The event topic. Carried as a string on the Rust side (the backend
    /// passes `DomainEvent::topic` through verbatim); typed as the closed
    /// [`DomainEventKind`] union in TypeScript, which the drift test pins to
    /// the emitted set.
    #[ts(as = "DomainEventKind")]
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
