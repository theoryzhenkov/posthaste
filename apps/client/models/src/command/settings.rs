//! The settings write: replace the global settings document.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Content for [`crate::Command::UpdateSettings`]: the FULL settings
/// document, written whole (read-modify-write against the `appSettings`
/// query — the service stores the document as one unit, so a sparse patch
/// would be a fiction). The document carries presentation and policy only;
/// no credential is representable in it.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsIntent {
    #[ts(as = "crate::mirror::AppSettings")]
    pub settings: domain::AppSettings,
    /// Transient command flag (not persisted state): when true, the backend
    /// re-runs the backfill-enabled automation rules against existing mail
    /// after saving.
    #[serde(default)]
    pub force_backfill: bool,
}
