//! The sync-now intent: trigger one account's sync cycle on demand.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Content for [`crate::Command::SyncNow`]. Progress and completion surface
/// as events and account status, like any scheduled sync.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SyncNowIntent {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    /// Sync depth; absent means incremental.
    #[serde(default)]
    #[ts(optional = nullable, as = "Option<crate::mirror::SyncMode>")]
    pub mode: Option<domain::SyncMode>,
}
