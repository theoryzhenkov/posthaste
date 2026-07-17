//! Automation-rule intents: create, replace, delete. The rule list is read
//! from the settings document (`appSettings` query); previewing a condition
//! is the `automationRulePreview` query.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Content for [`crate::Command::CreateAutomationRule`]. The rule carries
/// its own (client-minted) id.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CreateAutomationRuleIntent {
    #[ts(as = "crate::mirror::AutomationRule")]
    pub rule: domain::AutomationRule,
}

/// Content for [`crate::Command::UpdateAutomationRule`]: a full replacement,
/// keyed by `rule.id`.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAutomationRuleIntent {
    #[ts(as = "crate::mirror::AutomationRule")]
    pub rule: domain::AutomationRule,
}

/// Target for [`crate::Command::DeleteAutomationRule`].
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAutomationRuleIntent {
    pub rule_id: String,
}
