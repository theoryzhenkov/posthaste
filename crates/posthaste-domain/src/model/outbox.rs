//! Local-first command outbox model.
//!
//! This is the shared abstraction every tier (client TS, runtime Rust) conforms
//! to so the two implementations cannot drift: one operation envelope, one
//! lifecycle state machine, one conflict policy. The types are exposed through
//! the OpenAPI schema and regenerated into the client, which mirrors the same
//! state machine against the same envelope.
//!
//! A command is applied to the local store immediately, persisted as an
//! [`Operation`], and flushed to the next tier (runtime -> provider, or
//! client -> runtime) when reachable. `id` is the idempotency key across every
//! tier; `depends_on` preserves per-entity ordering.
//!
//! @spec docs/L1-outbox#operation-model

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::AccountId;

string_id!(
    /// Client-minted, globally-stable identifier for an outbox operation.
    ///
    /// Used as the idempotency key across every tier (client, runtime, and the
    /// runtime's own record of what it has pushed to the provider). A tier must
    /// never apply the same `OperationId` twice.
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

/// Lifecycle state of an operation within a single tier's outbox.
///
/// ```text
/// pending ─▶ inflight ─▶ applied
///    ▲          │  │ └──▶ failed
///    └──────────┘  └────▶ conflicted ─▶ inflight (after resolution)
/// ```
///
/// @spec docs/L1-outbox#state-machine
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum OperationState {
    /// Persisted locally and applied optimistically; not yet sent to the next tier.
    Pending,
    /// Currently being flushed to the next tier.
    Inflight,
    /// Accepted/applied by the next tier; awaiting prune.
    Applied,
    /// Base version diverged from the next tier; needs policy resolution.
    Conflicted,
    /// Permanently failed (e.g. validation); surfaced to the user.
    Failed,
}

impl OperationState {
    /// Whether the operation has reached a terminal state (no further flush).
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Applied | Self::Failed)
    }

    /// Whether the operation is eligible to be flushed to the next tier.
    pub fn is_flushable(self) -> bool {
        matches!(self, Self::Pending | Self::Conflicted)
    }

    /// Validate a lifecycle transition. The same matrix is mirrored on the
    /// client so both tiers agree on what is reachable.
    ///
    /// @spec docs/L1-outbox#state-machine
    pub fn can_transition_to(self, next: Self) -> bool {
        use OperationState::*;
        match (self, next) {
            // Begin a flush.
            (Pending, Inflight) => true,
            // Flush resolved one way or another.
            (Inflight, Applied | Conflicted | Failed) => true,
            // Transient failure returns the op to the queue for retry.
            (Inflight, Pending) => true,
            // A conflicted op is re-flushed once resolution rewrites its base.
            (Conflicted, Inflight) => true,
            (Conflicted, Applied | Failed) => true,
            // Idempotent self-transition (re-applying the same state) is allowed.
            _ => self == next,
        }
    }
}

/// How a flush conflict (base-version divergence at the next tier) is resolved.
///
/// @spec docs/L1-outbox#conflict-policy
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ConflictPolicy {
    /// Local edit wins; overwrite the next tier's state. Drafts use this: the
    /// author's in-progress edit is authoritative.
    LocalWins,
    /// Refresh from the next tier, keep the optimistic value, and surface only
    /// on true divergence. Mirrors today's `StateMismatch` refresh path.
    RefreshAndKeep,
}

impl OperationKind {
    /// The conflict policy for this op kind. Encoded in the shared model so both
    /// tiers resolve conflicts identically.
    ///
    /// @spec docs/L1-outbox#conflict-policy
    pub fn conflict_policy(self) -> ConflictPolicy {
        match self {
            Self::DraftCreate | Self::DraftUpdate | Self::DraftDelete | Self::Send => {
                ConflictPolicy::LocalWins
            }
            Self::SetKeywords | Self::ReplaceMailboxes | Self::Destroy => {
                ConflictPolicy::RefreshAndKeep
            }
        }
    }

    /// Whether this op creates a new entity (and therefore has no base cursor and
    /// may carry a client-minted temp entity id to reconcile on first flush).
    pub fn creates_entity(self) -> bool {
        matches!(self, Self::DraftCreate)
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
/// The same envelope is persisted and flushed at every tier. `id` provides
/// cross-tier idempotency; `depends_on` preserves per-entity ordering so a
/// `draft.update` never flushes before the `draft.create` it builds on.
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
    /// envelope stays uniform across op kinds and tiers.
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub payload: Value,
    /// Optimistic-concurrency token captured when the op was enqueued, used to
    /// detect drift at flush time. `None` for entity-creating ops.
    pub base_cursor: Option<String>,
    pub state: OperationState,
    pub attempts: u32,
    pub last_error: Option<String>,
    /// The operation that must settle before this one (per-entity ordering).
    pub depends_on: Option<OperationId>,
    pub created_at: String,
    pub updated_at: String,
}

/// Terminal outcome of an operation flush, propagated to the next tier / UI via
/// the `operation.settled` event so optimistic state can be cleared or a
/// conflict surfaced.
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
    Conflicted,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flushable_states_are_pending_and_conflicted() {
        assert!(OperationState::Pending.is_flushable());
        assert!(OperationState::Conflicted.is_flushable());
        assert!(!OperationState::Inflight.is_flushable());
        assert!(!OperationState::Applied.is_flushable());
        assert!(!OperationState::Failed.is_flushable());
    }

    #[test]
    fn terminal_states_are_applied_and_failed() {
        assert!(OperationState::Applied.is_terminal());
        assert!(OperationState::Failed.is_terminal());
        assert!(!OperationState::Pending.is_terminal());
        assert!(!OperationState::Inflight.is_terminal());
        assert!(!OperationState::Conflicted.is_terminal());
    }

    #[test]
    fn state_machine_allows_only_defined_transitions() {
        use OperationState::*;
        // Happy path.
        assert!(Pending.can_transition_to(Inflight));
        assert!(Inflight.can_transition_to(Applied));
        // Retry + conflict resolution.
        assert!(Inflight.can_transition_to(Pending));
        assert!(Inflight.can_transition_to(Conflicted));
        assert!(Conflicted.can_transition_to(Inflight));
        // Disallowed shortcuts.
        assert!(!Pending.can_transition_to(Applied));
        assert!(!Applied.can_transition_to(Inflight));
        assert!(!Failed.can_transition_to(Inflight));
        // Idempotent self-transition.
        assert!(Applied.can_transition_to(Applied));
    }

    #[test]
    fn conflict_policy_matches_op_kind() {
        assert_eq!(
            OperationKind::DraftUpdate.conflict_policy(),
            ConflictPolicy::LocalWins
        );
        assert_eq!(
            OperationKind::Send.conflict_policy(),
            ConflictPolicy::LocalWins
        );
        assert_eq!(
            OperationKind::SetKeywords.conflict_policy(),
            ConflictPolicy::RefreshAndKeep
        );
        assert_eq!(
            OperationKind::Destroy.conflict_policy(),
            ConflictPolicy::RefreshAndKeep
        );
    }

    #[test]
    fn only_draft_create_creates_an_entity() {
        assert!(OperationKind::DraftCreate.creates_entity());
        assert!(!OperationKind::DraftUpdate.creates_entity());
        assert!(!OperationKind::SetKeywords.creates_entity());
    }
}
