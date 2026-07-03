//! Tier-2 local-first command outbox model.
//!
//! Operations are persisted intent between the runtime and provider. Pending
//! operations are a read-time overlay on top of the authoritative provider
//! projection; sync remains the only writer of that projection.
//!
//! @spec docs/L1-outbox#operation-model

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::AccountId;

string_id!(
    /// Runtime-minted, globally-stable identifier for an outbox operation.
    ///
    /// Used as the idempotency key for the runtime/provider boundary. The
    /// runtime must never push the same settled `OperationId` twice.
    ///
    /// @spec docs/L1-outbox#idempotency
    OperationId
);

/// The kind of mutation an operation carries.
///
/// @spec docs/L1-outbox#operation-model
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum OperationKind {
    SetKeywords,
    ReplaceMailboxes,
    Destroy,
    DraftCreate,
    DraftUpdate,
    DraftDelete,
    Send,
}

/// Lifecycle state of an operation within the runtime/provider outbox.
///
/// ```text
/// pending ─▶ inflight ─▶ applied ─▶ (retired/removed on convergence)
///    ▲          │  ├──▶ failed
///    │          │  └──▶ dispatchUncertain (send only)
///    └──────────┴────────────┘  (explicit user retry re-arms)
/// ```
///
/// @spec docs/L1-outbox#state-machine
/// @spec docs/replication/L1#retire-on-confirmation
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum OperationState {
    /// Persisted locally and reflected through the read-time overlay.
    Pending,
    /// Currently being flushed to the provider.
    Inflight,
    /// Accepted by the provider. A message state assertion rests here, folded
    /// by the read-time overlay, until a sync observes its effect into the
    /// projection and retires (removes) it; entity ops (drafts/sends) are pruned
    /// on flush instead. See [`OperationState::is_flushable`] and the
    /// retire-on-confirmation rule in `docs/replication/L1`.
    Applied,
    /// Permanently failed (e.g. validation); surfaced to the user.
    Failed,
    /// A **send** whose delivery outcome is unknown: the request timed out or
    /// the connection was lost after the submission may already have committed
    /// server-side, or a prior flush was interrupted mid-send. Removed from the
    /// auto-flush set (RFC-L2 D86) — a possibly-delivered message is **never**
    /// blind-resent. It rests here, surfaced as needs-attention, until the user
    /// explicitly retries (re-arms to `pending`, re-dispatched under the same
    /// idempotency identity — D84/D85) or discards it.
    ///
    /// @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
    DispatchUncertain,
}

impl OperationState {
    /// Whether the operation has reached a resting state that will not change
    /// without an explicit user action. `Failed` and `DispatchUncertain` both
    /// rest until the user retries or discards them. `Applied` is **not**
    /// resting — it is awaiting confirmation and is removed once a sync confirms
    /// its effect into the projection.
    ///
    /// @spec docs/replication/L1#retire-on-confirmation
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Failed | Self::DispatchUncertain)
    }

    /// Whether the operation is parked awaiting the user's confirm/discard
    /// because its delivery outcome is unknown (a possibly-delivered send).
    ///
    /// @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
    pub fn is_dispatch_uncertain(self) -> bool {
        matches!(self, Self::DispatchUncertain)
    }

    /// Whether the operation is eligible to be flushed to the provider.
    ///
    /// `Inflight` is included so a process crash after marking an operation
    /// inflight does not wedge the outbox forever on restart.
    pub fn is_flushable(self) -> bool {
        matches!(self, Self::Pending | Self::Inflight)
    }

    /// Validate a lifecycle transition.
    ///
    /// @spec docs/L1-outbox#state-machine
    pub fn can_transition_to(self, next: Self) -> bool {
        use OperationState::*;
        match (self, next) {
            // Begin a flush.
            (Pending, Inflight) => true,
            // Flush resolved one way or another.
            (Inflight, Applied | Failed) => true,
            // Transient failure returns the op to the queue.
            (Inflight, Pending) => true,
            // A send whose outcome is unknown parks instead of failing (D86);
            // a crashed-inflight send is likewise parked, not blind-resent.
            (Inflight, DispatchUncertain) => true,
            // Explicit user retry re-arms a failed or parked op to the queue.
            (Failed | DispatchUncertain, Pending) => true,
            // Idempotent self-transition (re-applying the same state) is allowed.
            _ => self == next,
        }
    }
}

impl OperationKind {
    /// Whether this op creates a new entity and may carry a client-minted temp
    /// entity id to reconcile on first flush.
    pub fn creates_entity(self) -> bool {
        matches!(self, Self::DraftCreate)
    }

    /// Whether this op is an idempotent message-state assertion.
    pub fn is_state_assertion(self) -> bool {
        matches!(
            self,
            Self::SetKeywords | Self::ReplaceMailboxes | Self::Destroy
        )
    }
}

/// The kind of entity an operation targets.
///
/// @spec docs/L1-outbox#operation-model
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum OperationEntityKind {
    Message,
    Draft,
}

/// The entity an operation targets.
///
/// `id` may be a client-minted temporary id until the entity is reconciled to a
/// provider id on first successful flush (see [`OperationSettlement::assigned_entity_id`]).
///
/// @spec docs/L1-outbox#temp-id-reconciliation
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OperationEntity {
    pub kind: OperationEntityKind,
    pub id: String,
}

/// A single local-first command.
///
/// `id` provides runtime/provider idempotency. `depends_on` preserves draft
/// chains only; state assertions coalesce instead of depending on earlier
/// assertions.
///
/// @spec docs/L1-outbox#operation-model
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Operation {
    pub id: OperationId,
    pub account_id: AccountId,
    pub entity: OperationEntity,
    pub kind: OperationKind,
    /// Kind-specific payload (the wrapped command or draft body), as JSON so the
    /// envelope stays uniform across kinds.
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub payload: Value,
    pub state: OperationState,
    pub attempts: u32,
    pub last_error: Option<String>,
    /// The operation that must settle before this one (draft-chain ordering).
    pub depends_on: Option<OperationId>,
    pub created_at: String,
    pub updated_at: String,
}

/// Terminal outcome of an operation flush, propagated via
/// `operation.settled` so optimistic state can be cleared or a failure surfaced.
///
/// @spec docs/L1-outbox#settlement
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OperationSettlement {
    pub id: OperationId,
    pub outcome: OperationOutcome,
    /// Set when a temp entity id was reconciled to a provider id on this flush.
    pub assigned_entity_id: Option<String>,
    pub error: Option<String>,
}

/// Outcome variants reported in [`OperationSettlement`].
///
/// @spec docs/L1-outbox#settlement
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum OperationOutcome {
    Applied,
    Failed,
}

/// Emitted on `operation.dispatch_uncertain` when a send is parked because its
/// delivery outcome is unknown (RFC-L2 D86/D87). Unlike a settlement, this is
/// **not** terminal — it is a needs-attention signal: the send may or may not
/// have reached the recipient, and the user must confirm (retry) or discard.
///
/// @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OperationDispatchUncertain {
    pub id: OperationId,
    /// Why the delivery is uncertain (e.g. "send timed out; delivery uncertain").
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_and_inflight_operations_are_flushable() {
        assert!(OperationState::Pending.is_flushable());
        assert!(OperationState::Inflight.is_flushable());
        assert!(!OperationState::Applied.is_flushable());
        assert!(!OperationState::Failed.is_flushable());
        // A parked send is never auto-flushed (D86).
        assert!(!OperationState::DispatchUncertain.is_flushable());
    }

    #[test]
    fn failed_and_parked_rest_awaiting_the_user() {
        assert!(OperationState::Failed.is_terminal());
        // A parked send rests until the user retries or discards it.
        assert!(OperationState::DispatchUncertain.is_terminal());
        assert!(OperationState::DispatchUncertain.is_dispatch_uncertain());
        // Applied rests folded, awaiting convergence, then is retired — not
        // terminal.
        assert!(!OperationState::Applied.is_terminal());
        assert!(!OperationState::Pending.is_terminal());
        assert!(!OperationState::Inflight.is_terminal());
        assert!(!OperationState::Failed.is_dispatch_uncertain());
    }

    #[test]
    fn state_machine_allows_only_defined_transitions() {
        use OperationState::*;
        // Happy path.
        assert!(Pending.can_transition_to(Inflight));
        assert!(Inflight.can_transition_to(Applied));
        // Transient retry.
        assert!(Inflight.can_transition_to(Pending));
        // A possibly-delivered send parks instead of failing (D86).
        assert!(Inflight.can_transition_to(DispatchUncertain));
        // Explicit user retry re-arms a parked or failed op.
        assert!(DispatchUncertain.can_transition_to(Pending));
        assert!(Failed.can_transition_to(Pending));
        // Disallowed shortcuts.
        assert!(!Pending.can_transition_to(Applied));
        assert!(!Applied.can_transition_to(Inflight));
        assert!(!Failed.can_transition_to(Inflight));
        assert!(!DispatchUncertain.can_transition_to(Applied));
        // Idempotent self-transition.
        assert!(Applied.can_transition_to(Applied));
    }

    #[test]
    fn only_draft_create_creates_an_entity() {
        assert!(OperationKind::DraftCreate.creates_entity());
        assert!(!OperationKind::DraftUpdate.creates_entity());
        assert!(!OperationKind::SetKeywords.creates_entity());
    }

    #[test]
    fn message_mutations_are_state_assertions() {
        assert!(OperationKind::SetKeywords.is_state_assertion());
        assert!(OperationKind::ReplaceMailboxes.is_state_assertion());
        assert!(OperationKind::Destroy.is_state_assertion());
        assert!(!OperationKind::DraftUpdate.is_state_assertion());
        assert!(!OperationKind::Send.is_state_assertion());
    }
}
