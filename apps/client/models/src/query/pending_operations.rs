//! The pending-operations family: the outbox with verdicts.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Read the outbox — pending intents and their verdicts — optionally for one
/// account.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingOperationsQuery {
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::AccountId>")]
    pub account_id: Option<domain::AccountId>,
}

/// One outbox operation as the client renders it: what it is, what it
/// targets, where it stands. Payload bodies stay in the backend.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingOperationRow {
    #[ts(as = "crate::mirror::OperationId")]
    pub id: domain::OperationId,
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::OperationKind")]
    pub kind: domain::OperationKind,
    #[ts(as = "crate::mirror::OperationState")]
    pub state: domain::OperationState,
    #[ts(as = "crate::mirror::OperationEntityKind")]
    pub entity_kind: domain::OperationEntityKind,
    /// The targeted message or draft id (possibly still a client-minted temp
    /// id awaiting provider reconciliation).
    pub entity_id: String,
    pub attempts: u32,
    pub last_error: Option<String>,
    /// Earliest submission time for a scheduled send (RFC 3339); absent for
    /// everything else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub send_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// The outbox in scope, newest first.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingOperationsResult {
    pub rows: Vec<PendingOperationRow>,
}
