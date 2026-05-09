use super::*;

const DEFAULT_AUTOMATION_RULE_PREVIEW_LIMIT: usize = 5;
const MAX_AUTOMATION_RULE_PREVIEW_LIMIT: usize = 50;

fn automation_rule_preview_limit(limit: Option<usize>) -> Result<usize, ApiError> {
    let limit = limit.unwrap_or(DEFAULT_AUTOMATION_RULE_PREVIEW_LIMIT);
    if limit == 0 || limit > MAX_AUTOMATION_RULE_PREVIEW_LIMIT {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_limit",
            format!(
                "limit must be between 1 and {MAX_AUTOMATION_RULE_PREVIEW_LIMIT} preview messages"
            ),
        ));
    }
    Ok(limit)
}

fn normalize_cache_policy(mut policy: CachePolicy) -> CachePolicy {
    policy.hard_cap_bytes = policy.hard_cap_bytes.max(policy.soft_cap_bytes);
    policy
}

fn normalize_telemetry_settings(
    mut telemetry: TelemetrySettings,
) -> Result<TelemetrySettings, ApiError> {
    match telemetry.mode {
        TelemetryMode::Off => {
            telemetry.notice_version = None;
            telemetry.enabled_at = None;
            telemetry.categories.clear();
        }
        TelemetryMode::Aggregate | TelemetryMode::Product => {
            if telemetry
                .notice_version
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
                || telemetry
                    .enabled_at
                    .as_ref()
                    .is_none_or(|value| value.trim().is_empty())
                || telemetry.categories.is_empty()
                || telemetry.categories.iter().any(|category| {
                    !matches!(
                        category.as_str(),
                        "health" | "performance" | "cache" | "ui" | "profile"
                    )
                })
            {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_telemetry_consent",
                    "telemetry opt-in requires a notice version, timestamp, and approved categories",
                ));
            }
        }
    }
    Ok(telemetry)
}

/// GET /v1/settings
///
/// @spec docs/L1-api#settings
pub async fn get_settings(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AppSettings>, ApiError> {
    state
        .service
        .get_app_settings()
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// PATCH /v1/settings
///
/// Validates that the referenced default account exists before persisting.
///
/// @spec docs/L1-api#settings
pub async fn patch_settings(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PatchSettingsRequest>,
) -> Result<Json<AppSettings>, ApiError> {
    let mut settings = state
        .service
        .get_app_settings()
        .map_err(ApiError::from_service_error)?;
    if let Some(default_account_id) = &request.default_account_id {
        if let Some(default_account_id) = default_account_id {
            let account = state
                .service
                .get_source(&AccountId::from(default_account_id.as_str()))
                .map_err(ApiError::from_service_error)?;
            if account.is_none() {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_account",
                    "default account must reference an existing account",
                ));
            }
            settings.default_account_id = Some(AccountId::from(default_account_id.as_str()));
        } else {
            settings.default_account_id = None;
        }
    }
    if let Some(automation_rules) = &request.automation_rules {
        settings.automation_rules = normalize_automation_rules(automation_rules);
    }
    if let Some(automation_drafts) = &request.automation_drafts {
        settings.automation_drafts = normalize_automation_rules(automation_drafts);
    }
    if let Some(cache_policy) = request.cache_policy {
        settings.cache_policy = normalize_cache_policy(cache_policy);
    }
    let telemetry_mode_was_set_to_off = request
        .telemetry
        .as_ref()
        .is_some_and(|telemetry| telemetry.mode == TelemetryMode::Off);
    if let Some(telemetry) = request.telemetry {
        settings.telemetry = normalize_telemetry_settings(telemetry)?;
    }
    validate_automation_rules(&settings.automation_rules)?;
    validate_automation_drafts(&settings.automation_rules, &settings.automation_drafts)?;
    state
        .service
        .put_app_settings(&settings)
        .map_err(ApiError::from_service_error)?;
    if telemetry_mode_was_set_to_off {
        TelemetrySpool::purge_root(&state.telemetry_root).map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "telemetry_purge_failed",
                format!("failed to delete pending telemetry: {error}"),
            )
        })?;
    }
    if request.automation_rules.is_some() {
        state
            .service
            .ensure_automation_backfills_for_current_rules()
            .map_err(ApiError::from_service_error)?;
    }
    Ok(Json(settings))
}

/// POST /v1/automation-rules:preview
///
/// Returns a small newest-first sample and total count for a draft rule
/// condition using the same indexed rule query path as smart mailboxes.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub async fn preview_automation_rule(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PreviewAutomationRuleRequest>,
) -> Result<Json<AutomationRulePreviewResponse>, ApiError> {
    let limit = automation_rule_preview_limit(request.limit)?;
    let (_, total) = state
        .service
        .count_messages_by_rule(&request.condition)
        .map_err(ApiError::from_service_error)?;
    let page = state
        .service
        .query_message_page_by_rule(
            &request.condition,
            limit,
            None,
            MessageSortField::Date,
            SortDirection::Desc,
        )
        .map_err(ApiError::from_service_error)?;
    Ok(Json(AutomationRulePreviewResponse {
        total,
        items: page.items,
    }))
}
