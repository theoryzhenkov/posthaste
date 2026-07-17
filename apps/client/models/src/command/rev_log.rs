//! Undo/redo intents: move one account's rev-log cursor. The log itself is
//! read through the `revLog` query.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Content for [`crate::Command::Undo`]: revert the step at the account's
/// rev-log cursor and move the cursor down one.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UndoIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
}

/// Content for [`crate::Command::Redo`]: re-apply the most recently undone
/// step and move the cursor up one.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RedoIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
}
