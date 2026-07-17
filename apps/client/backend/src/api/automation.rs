//! The automation family: rule CRUD against the global settings document
//! (the rule list is read through the `appSettings` query) and the
//! rule-condition preview over today's mail.

use axum::http::StatusCode;
use posthaste_client_models::{
    ApiErrorKind, AutomationRulePreviewQuery, AutomationRulePreviewResult,
    CreateAutomationRuleIntent, DeleteAutomationRuleIntent, UpdateAutomationRuleIntent,
};
use posthaste_domain_model::{
    AccountId, AppSettings, DomainEvent, MessageSortField, SortDirection,
    EVENT_TOPIC_SETTINGS_UPDATED,
};
use posthaste_domain_service::{validate_automation_drafts, validate_automation_rules};

use super::{now_rfc3339, ApiFailure, DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT};
use crate::AppState;

/// The settings document is global, but every domain event names an account;
/// settings events carry this pseudo id.
const SETTINGS_EVENT_ACCOUNT_ID: &str = "app";

pub(crate) fn evaluate_rule_preview(
    app: &AppState,
    query: AutomationRulePreviewQuery,
) -> Result<AutomationRulePreviewResult, ApiFailure> {
    let limit = query
        .limit
        .map(|limit| limit as usize)
        .unwrap_or(DEFAULT_LIST_LIMIT)
        .min(MAX_LIST_LIMIT);
    let (_, total) = app.service.count_messages_by_rule(&query.condition)?;
    let page = app.service.query_message_page_by_rule(
        &query.condition,
        limit,
        None,
        MessageSortField::Date,
        SortDirection::Desc,
    )?;
    Ok(AutomationRulePreviewResult {
        total,
        rows: page.items,
    })
}

pub(crate) fn create_rule(
    app: &AppState,
    intent: CreateAutomationRuleIntent,
) -> Result<u64, ApiFailure> {
    if intent.rule.id.trim().is_empty() {
        return Err(ApiFailure::malformed(
            "automation rule id must not be empty",
        ));
    }
    let mut settings = app.service.get_app_settings()?;
    if settings
        .automation_rules
        .iter()
        .any(|existing| existing.id == intent.rule.id)
    {
        return Err(ApiFailure::new(
            StatusCode::CONFLICT,
            ApiErrorKind::Conflict,
            format!("automation rule {} already exists", intent.rule.id),
            false,
        ));
    }
    settings.automation_rules.push(intent.rule);
    save_rules(app, settings)
}

pub(crate) fn update_rule(
    app: &AppState,
    intent: UpdateAutomationRuleIntent,
) -> Result<u64, ApiFailure> {
    let mut settings = app.service.get_app_settings()?;
    let slot = settings
        .automation_rules
        .iter_mut()
        .find(|existing| existing.id == intent.rule.id)
        .ok_or_else(|| ApiFailure::unknown_id(format!("automation rule {}", intent.rule.id)))?;
    *slot = intent.rule;
    save_rules(app, settings)
}

pub(crate) fn delete_rule(
    app: &AppState,
    intent: DeleteAutomationRuleIntent,
) -> Result<u64, ApiFailure> {
    let mut settings = app.service.get_app_settings()?;
    let before = settings.automation_rules.len();
    settings
        .automation_rules
        .retain(|existing| existing.id != intent.rule_id);
    if settings.automation_rules.len() == before {
        // Idempotent: the rule is already gone, which is the requested state.
        return Ok(app.events.generation());
    }
    save_rules(app, settings)
}

/// Validate and persist the whole document, refresh the durable backfill
/// jobs for the (possibly changed) ruleset, and announce the change on the
/// stream. One shared finish path so every rule mutation behaves alike.
fn save_rules(app: &AppState, settings: AppSettings) -> Result<u64, ApiFailure> {
    validate_automation_rules(&settings.automation_rules)
        .map_err(|error| ApiFailure::malformed(error.to_string()))?;
    validate_automation_drafts(&settings.automation_rules, &settings.automation_drafts)
        .map_err(|error| ApiFailure::malformed(error.to_string()))?;
    app.service.put_app_settings(&settings)?;
    // Backfill-eligible rules also apply to existing mail: make sure every
    // enabled account holds a durable job for the current ruleset (cheap
    // when the fingerprint is unchanged).
    app.service
        .ensure_automation_backfills_for_current_rules()?;
    app.events.publish(&[DomainEvent {
        seq: 0,
        account_id: AccountId::from(SETTINGS_EVENT_ACCOUNT_ID),
        topic: EVENT_TOPIC_SETTINGS_UPDATED.to_string(),
        occurred_at: now_rfc3339(),
        mailbox_id: None,
        message_id: None,
        payload: serde_json::json!({ "changed": ["automationRules"] }),
    }]);
    Ok(app.events.generation())
}
