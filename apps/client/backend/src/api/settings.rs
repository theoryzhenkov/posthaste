//! The settings family: the global settings document, read and written
//! whole. The document carries presentation and policy only; no credential
//! is representable in it.

use posthaste_client_models::{AppSettingsQuery, AppSettingsResult, UpdateSettingsIntent};
use posthaste_domain_model::{
    AccountId, AppSettings, ComposeSettings, DomainEvent, EVENT_TOPIC_SETTINGS_UPDATED,
};
use posthaste_domain_service::{validate_automation_drafts, validate_automation_rules};

use super::{now_rfc3339, ApiFailure};
use crate::AppState;

/// The synthetic account id global (non-account) events are published under.
const GLOBAL_EVENT_ACCOUNT_ID: &str = "app";

pub(crate) fn evaluate_app_settings(
    app: &AppState,
    _query: AppSettingsQuery,
) -> Result<AppSettingsResult, ApiFailure> {
    Ok(AppSettingsResult {
        settings: app.service.get_app_settings()?,
    })
}

/// Replace the whole settings document (read-modify-write against the
/// `appSettings` query — the service stores the document as one unit). The
/// transient `forceBackfill` flag re-runs the backfill-enabled automation
/// rules against existing mail; otherwise a changed ruleset only ensures its
/// durable backfill job exists.
pub(crate) fn update_settings(
    app: &AppState,
    intent: UpdateSettingsIntent,
) -> Result<u64, ApiFailure> {
    let UpdateSettingsIntent {
        settings,
        force_backfill,
    } = intent;
    validate_settings_document(app, &settings)?;
    let previous = app.service.get_app_settings()?;
    app.service.put_app_settings(&settings)?;
    if force_backfill {
        app.service.reset_automation_backfills_for_current_rules()?;
    } else if previous.automation_rules != settings.automation_rules {
        app.service
            .ensure_automation_backfills_for_current_rules()?;
    }
    Ok(publish_settings_event(app))
}

fn validate_settings_document(app: &AppState, settings: &AppSettings) -> Result<(), ApiFailure> {
    if let Some(default_account_id) = &settings.default_account_id {
        if app.service.get_source(default_account_id)?.is_none() {
            return Err(ApiFailure::malformed(format!(
                "defaultAccountId {} does not name a configured account",
                default_account_id.as_str()
            )));
        }
    }
    validate_automation_rules(&settings.automation_rules)
        .map_err(|error| ApiFailure::malformed(error.message().to_string()))?;
    validate_automation_drafts(&settings.automation_rules, &settings.automation_drafts)
        .map_err(|error| ApiFailure::malformed(error.message().to_string()))?;
    if let Some(compose) = &settings.compose {
        if let Some(delay) = compose.undo_send_delay_seconds {
            // A huge silent hold would read as mail loss — long waits belong
            // to the explicit send-later schedule.
            if delay > ComposeSettings::MAX_UNDO_SEND_DELAY_SECONDS {
                return Err(ApiFailure::malformed(format!(
                    "undoSendDelaySeconds must be at most {}",
                    ComposeSettings::MAX_UNDO_SEND_DELAY_SECONDS
                )));
            }
        }
    }
    Ok(())
}

/// Publish the settings-updated event (bumping the generation) and return
/// the resulting generation.
fn publish_settings_event(app: &AppState) -> u64 {
    app.events.publish(&[DomainEvent {
        seq: 0,
        account_id: AccountId::from(GLOBAL_EVENT_ACCOUNT_ID),
        topic: EVENT_TOPIC_SETTINGS_UPDATED.to_string(),
        occurred_at: now_rfc3339(),
        mailbox_id: None,
        message_id: None,
        payload: serde_json::json!({ "scope": "app" }),
    }]);
    app.events.generation()
}
