use super::*;

/// Account-level automation rule evaluated by authority server triggers.
///
/// @spec docs/L1-accounts#toml-schema
/// @spec docs/L1-sync#automation-actions
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AutomationRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub triggers: Vec<AutomationTrigger>,
    pub condition: SmartMailboxRule,
    pub actions: Vec<AutomationAction>,
    pub backfill: bool,
}

/// Durable state for authority-server-owned automation backfill work.
///
/// @spec docs/L1-sync#automation-actions
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationBackfillJob {
    pub account_id: AccountId,
    pub rule_fingerprint: String,
    pub status: AutomationBackfillJobStatus,
    pub attempts: i64,
    pub last_error: Option<String>,
    pub updated_at: String,
}

/// Lifecycle state for an automation backfill job.
///
/// @spec docs/L1-sync#automation-actions
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AutomationBackfillJobStatus {
    Pending,
    Completed,
}

/// Result of one durable automation backfill worker batch.
///
/// @spec docs/L1-sync#automation-actions
#[derive(Clone, Debug)]
pub struct AutomationBackfillBatchOutcome {
    pub ran: bool,
    pub events: Vec<DomainEvent>,
    pub has_more: bool,
}

/// Result of one optional-content cache worker batch.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Default)]
pub struct CacheWorkerBatchOutcome {
    pub scanned: usize,
    pub attempted: usize,
    pub attempted_bytes: u64,
    pub cached: usize,
    pub cached_bytes: u64,
    pub failed: usize,
    pub skipped: usize,
    pub events: Vec<DomainEvent>,
    /// True when the batch stopped early because it hit its own wall-clock
    /// budget (`BODY_CACHE_BATCH_BUDGET`) — partial work was done and the
    /// remaining candidates stay `wanted` for a later, backed-off batch.
    pub deadline_exceeded: bool,
}

/// Result of one optional-content cache re-score batch.
///
/// @spec docs/L1-sync#local-cache-planning
#[derive(Clone, Debug, Default)]
pub struct CacheRescoreBatchOutcome {
    pub scanned: usize,
    pub updated: usize,
    pub skipped: usize,
}

/// Event types that can cause an automation rule to run.
///
/// @spec docs/L1-sync#automation-actions
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AutomationTrigger {
    MessageArrived,
    MessageChanged,
    Manual,
}

/// Supported effects for automation rules.
///
/// @spec docs/L1-sync#automation-actions
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AutomationAction {
    ApplyTag {
        tag: String,
    },
    RemoveTag {
        tag: String,
    },
    MarkRead,
    MarkUnread,
    Flag,
    Unflag,
    MoveToMailbox {
        #[cfg_attr(feature = "openapi", schema(rename = "mailboxId"))]
        mailbox_id: MailboxId,
    },
}
