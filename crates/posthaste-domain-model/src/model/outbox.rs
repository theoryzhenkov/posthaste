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

use super::commands::{ReplaceMailboxesCommand, SendMessageRequest, SetKeywordsCommand};
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
/// pending ─▶ inflight ─▶ applied ─▶ (removed by causal truncation)
///    ▲          │  ├──▶ failed
///    │          │  └──▶ dispatchUncertain (send only)
///    └──────────┴────────────┘  (explicit user retry re-arms)
/// ```
///
/// @spec docs/L1-outbox#state-machine
/// @spec docs/backend/L2-optimism#settlement-and-truncation
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum OperationState {
    /// Persisted locally and reflected through the read-time overlay.
    Pending,
    /// Currently being flushed to the provider.
    Inflight,
    /// Settled: the provider accepted the op. It rests in the log — still
    /// folded by replay (its effect keeps serving until base catches up),
    /// excluded from the flush lane (never re-delivered) and from the
    /// pendingOperations surface — until CAUSAL truncation removes it: for a
    /// JMAP settlement that captured a provider sync position, once the sync
    /// state chain reaches that watermark; otherwise, once a sync cycle that
    /// started after settlement completes. Both are pure ordering checks — no
    /// comparison of state decides retirement.
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
    /// resting — it is settled and leaves the log by causal truncation on a
    /// later sync cycle (see [`OperationState::Applied`]).
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

    /// Whether this op is a draft save (create or update) — the coalescing
    /// unit for D174 (same-key saves replace a still-queued save).
    pub fn is_draft_save(self) -> bool {
        matches!(self, Self::DraftCreate | Self::DraftUpdate)
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
/// `id` provides runtime/provider idempotency. There is no cross-operation
/// dependency edge (D174): state assertions coalesce, same-key draft saves
/// coalesce (last-writer-wins per compose session), and everything else
/// relies on the flusher's insertion-order drain.
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
    /// envelope stays uniform across kinds. Decode through
    /// [`Operation::intent`], never ad hoc (NS2 Slice 2).
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub payload: Value,
    /// D155 payload envelope version. v1 = the historical per-kind shapes.
    #[serde(default = "default_payload_version")]
    pub payload_version: i64,
    pub state: OperationState,
    pub attempts: u32,
    pub last_error: Option<String>,
    /// Scheduled-send hold (send ops only): the earliest flush time, normalized
    /// UTC whole-second RFC 3339. A queued op with `send_at` in the future is
    /// excluded from the flushable set (it rests `pending`, visible and
    /// discardable) until due; `None` (every non-send op, and an immediate
    /// send) keeps the pre-existing flush-now behavior. Persisted with the op,
    /// so a schedule survives restart. Local-first: the send fires on the first
    /// flush window at/after `send_at` while the app runs — not a server-side
    /// schedule.
    ///
    /// @spec docs/L1-outbox#operation-model
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_at: Option<String>,
    /// Undo-send hold deadline in the DAEMON's monotonic-anchored epoch
    /// seconds (D152): stamped and judged on one clock, meaningless across
    /// machines (deliberately not display data — clients show their own local
    /// countdown). `None` for immediate and wall-scheduled sends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold_until_mono: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// A settled (`applied`) operation with its truncation markers, as the store
/// returns it to the truncation pass. Backend-internal — never mirrored into
/// the client protocol (settled ops are invisible to the wire; they filter out
/// of pendingOperations exactly like a removed op).
#[derive(Clone, Debug)]
pub struct SettledOperation {
    pub operation: Operation,
    /// When the op settled, on the daemon's monotonic-anchored epoch-seconds
    /// clock (the clock that also stamps sync-cycle-start markers, so the
    /// "cycle started after settlement" ordering check is single-clock).
    /// `None` on a legacy row settled before the markers existed — treated as
    /// truncate-eligible on any completed cycle.
    pub settled_at_mono: Option<i64>,
    /// The provider sync position that includes the settled change (a JMAP
    /// `set` response's `newState`, in the stored cursor encoding). `None`
    /// when the provider named no usable position (IMAP; a JMAP mutation
    /// without a state) — the cycle rule alone truncates then.
    pub watermark: Option<String>,
}

/// How the provider filed the Sent copy of a DELIVERED send (D154). The full
/// send-outcome space is `Delivered { filed } | Uncertain(cause) | Failed`:
/// `Uncertain` rides `operation.dispatch_uncertain` (the park, D86) and
/// `Failed` rides the failed settlement — this enum types the piece that used
/// to be a warn-and-forget boolean (`moved_to_sent`).
///
/// @spec docs/eph/RFC-L2-send-draft-state-machine#3-decisions
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SendFiling {
    /// The Sent copy is filed (server-side move applied, Sent append
    /// succeeded, or the provider files sent mail itself).
    Filed,
    /// Delivery committed but the Sent copy is NOT confirmed filed (the
    /// server ignored the Drafts→Sent move; the IMAP Sent append failed or no
    /// Sent mailbox was discovered). The provisional Sent overlay row stays
    /// confirmation-gated; reads "Sent — filing" (Slice 5 surfaces it).
    PendingFiling,
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
    /// For an APPLIED send: how the Sent copy was filed (D154). `None` on
    /// every non-send settlement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_filing: Option<SendFiling>,
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

/// The TYPED intent an outbox operation carries (NS2 Slice 2, D155/D171):
/// the single vocabulary behind the `(kind, payload)` persistence envelope.
///
/// [`MailIntent::from_parts`] is THE ONE decode boundary — every consumer
/// (flush dispatch, the overlay fold, coalescing, settlement effects) matches
/// on this enum instead of re-deriving meaning from JSON at its own site,
/// which is the divergence class behind the draft-identity bug family. The
/// persistence shape is unchanged (envelope v1 == the historical payload
/// bytes per kind), so existing outbox rows decode as-is.
#[derive(Clone, Debug)]
pub enum MailIntent {
    SetKeywords(SetKeywordsCommand),
    ReplaceMailboxes(ReplaceMailboxesCommand),
    Destroy,
    /// `create` distinguishes DraftCreate (mints an entity) from DraftUpdate.
    SaveDraft {
        create: bool,
        request: SendMessageRequest,
    },
    /// A draft removal: the send-consume settlement effect (idempotent
    /// redelivery masks provider `notFound`, D133) or a user discard (which
    /// surfaces it).
    DiscardDraft {
        idempotent_redelivery: bool,
    },
    Send(SendMessageRequest),
}

impl MailIntent {
    /// THE decode boundary: envelope `(kind, version, payload)` → typed
    /// intent. `version` is the D155 payload envelope version; only v1 (the
    /// historical shapes) exists today — an unknown version is a hard error
    /// (a newer build wrote it; refuse rather than misread).
    pub fn from_parts(
        kind: OperationKind,
        version: i64,
        payload: &serde_json::Value,
    ) -> Result<Self, String> {
        if version != 1 {
            return Err(format!(
                "unknown outbox payload envelope version {version} for {kind:?}"
            ));
        }
        let decode_error = |error: serde_json::Error| format!("invalid {kind:?} payload: {error}");
        Ok(match kind {
            OperationKind::SetKeywords => {
                Self::SetKeywords(serde_json::from_value(payload.clone()).map_err(decode_error)?)
            }
            OperationKind::ReplaceMailboxes => Self::ReplaceMailboxes(
                serde_json::from_value(payload.clone()).map_err(decode_error)?,
            ),
            OperationKind::Destroy => Self::Destroy,
            OperationKind::DraftCreate => Self::SaveDraft {
                create: true,
                request: serde_json::from_value(payload.clone()).map_err(decode_error)?,
            },
            OperationKind::DraftUpdate => Self::SaveDraft {
                create: false,
                request: serde_json::from_value(payload.clone()).map_err(decode_error)?,
            },
            OperationKind::DraftDelete => Self::DiscardDraft {
                idempotent_redelivery: payload
                    .get("idempotentRedelivery")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            },
            OperationKind::Send => {
                Self::Send(serde_json::from_value(payload.clone()).map_err(decode_error)?)
            }
        })
    }

    /// The persistence kind this intent serializes under.
    pub fn kind(&self) -> OperationKind {
        match self {
            Self::SetKeywords(_) => OperationKind::SetKeywords,
            Self::ReplaceMailboxes(_) => OperationKind::ReplaceMailboxes,
            Self::Destroy => OperationKind::Destroy,
            Self::SaveDraft { create: true, .. } => OperationKind::DraftCreate,
            Self::SaveDraft { create: false, .. } => OperationKind::DraftUpdate,
            Self::DiscardDraft { .. } => OperationKind::DraftDelete,
            Self::Send(_) => OperationKind::Send,
        }
    }
}

impl Operation {
    /// Decode this operation's typed intent (envelope v1 — see
    /// [`MailIntent::from_parts`]).
    pub fn intent(&self) -> Result<MailIntent, String> {
        MailIntent::from_parts(self.kind, self.payload_version, &self.payload)
    }
}

fn default_payload_version() -> i64 {
    1
}

/// The transport-shared send identity token derived from the outbox operation
/// id (D84/D85): both gateways stamp `<token>@<domain>` as the outgoing
/// RFC5322 `Message-ID` (and JMAP derives its create-ids from it), so every
/// retry of the same send carries the same identity. Sanitized to the JMAP
/// creation-id charset (the leading letters keep it valid on strict servers).
///
/// ONE derivation for the stamp side (gateways) and the match side (the
/// overlay's provisional Sent-row adoption, which matches base rows by
/// [`send_identity_prefix`] — domain-independent), so they cannot drift.
pub fn send_identity_token(operation_id: &str) -> String {
    let sanitized: String = operation_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    format!("phsend-{sanitized}")
}

/// The `Message-ID` prefix (`<token>@`) a send's provider copy carries in any
/// domain — the adoption key for the provisional Sent overlay row (NS2
/// Slice 4: reconcile-by-intent-id).
pub fn send_identity_prefix(operation_id: &str) -> String {
    format!("{}@", send_identity_token(operation_id))
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
    fn intent_decodes_every_kind_from_its_historical_payload_shape() {
        use serde_json::json;
        let cases = vec![
            (
                OperationKind::SetKeywords,
                json!({ "add": ["$flagged"], "remove": [] }),
            ),
            (
                OperationKind::ReplaceMailboxes,
                json!({ "mailboxIds": ["archive"] }),
            ),
            (OperationKind::Destroy, json!({})),
            (
                OperationKind::DraftDelete,
                json!({ "idempotentRedelivery": true }),
            ),
        ];
        for (kind, payload) in cases {
            let intent = MailIntent::from_parts(kind, 1, &payload)
                .unwrap_or_else(|error| panic!("{kind:?}: {error}"));
            assert_eq!(intent.kind(), kind, "kind round-trips for {kind:?}");
        }
        match MailIntent::from_parts(
            OperationKind::DraftDelete,
            1,
            &json!({ "idempotentRedelivery": true }),
        )
        .unwrap()
        {
            MailIntent::DiscardDraft {
                idempotent_redelivery,
            } => assert!(idempotent_redelivery),
            other => panic!("wrong intent: {other:?}"),
        }
    }

    #[test]
    fn intent_refuses_an_unknown_envelope_version() {
        let error = MailIntent::from_parts(OperationKind::Destroy, 2, &serde_json::json!({}))
            .expect_err("a future envelope version must refuse, not misread");
        assert!(error.contains("version 2"), "{error}");
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
