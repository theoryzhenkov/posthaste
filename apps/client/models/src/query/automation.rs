//! The automation family: previewing what a rule condition matches. The
//! rules themselves live in the settings document (`appSettings` query,
//! automation commands for CRUD).

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Evaluate an automation condition against today's mail: which messages
/// would it match right now, and how many in total.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRulePreviewQuery {
    /// The WHEN-clause to evaluate (the shared mail-query AST).
    #[ts(as = "crate::mirror::MailQueryRule")]
    pub condition: domain::MailQueryRule,
    /// Maximum sample rows to return; the backend applies (and caps to) its
    /// own default when absent.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub limit: Option<u32>,
}

/// The preview answer: the total match count plus a bounded sample.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRulePreviewResult {
    /// How many messages the condition matches in total.
    #[ts(type = "number")]
    pub total: i64,
    /// A sample of matching messages, newest first, bounded by the query's
    /// limit.
    #[ts(as = "Vec<crate::mirror::MessageSummary>")]
    pub rows: Vec<domain::MessageSummary>,
}
