//! Outbox-operation intents: retry and cancel, targeting rows of the
//! pending-operations query.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Target for [`crate::Command::RetryOperation`]: put a failed or parked
/// operation back in the flush queue.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RetryOperationIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::OperationId")]
    pub operation_id: domain::OperationId,
}

/// Target for [`crate::Command::CancelOperation`]: discard a pending
/// operation and roll back its overlay effect (a held send inside its undo
/// window cancels this way).
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CancelOperationIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::OperationId")]
    pub operation_id: domain::OperationId,
}
