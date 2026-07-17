//! The settings family: the global application settings document.
//!
//! The document carries appearance, notification prefs, the undo-send
//! default, mailbox colors, tag appearance, sidebar order, and the automation
//! rules — presentation and policy, never credentials. Secret material has no
//! representation in this family (see the dedicated `setAccountSecret`
//! command).

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Read the global settings document. Carries no parameters; it stays a
/// struct so adding a projection is not a wire break.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
pub struct AppSettingsQuery {}

/// The whole settings document, verbatim. The client edits it
/// read-modify-write through the `updateSettings` command.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsResult {
    #[ts(as = "crate::mirror::AppSettings")]
    pub settings: domain::AppSettings,
}
