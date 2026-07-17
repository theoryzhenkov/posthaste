//! The rev-log family: one account's undo/redo history and cursor.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Read one account's reversible-operation log with its undo/redo cursor.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RevLogQuery {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
}

/// The account's rev-log snapshot: steps in append order plus the cursor.
/// The `undo`/`redo` commands move the cursor; this query renders it.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RevLogResult {
    #[ts(as = "Vec<crate::mirror::RevLogStep>")]
    pub steps: Vec<domain::RevLogStep>,
    #[ts(as = "crate::mirror::RevCursor")]
    pub cursor: domain::RevCursor,
}
