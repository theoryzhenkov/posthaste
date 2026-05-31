use super::*;

const DEFAULT_AUTOMATION_RULE_PREVIEW_LIMIT: usize = 5;
const MAX_AUTOMATION_RULE_PREVIEW_LIMIT: usize = 50;

fn automation_rule_preview_limit(limit: Option<usize>) -> Result<usize, ApiError> {
    let limit = limit.unwrap_or(DEFAULT_AUTOMATION_RULE_PREVIEW_LIMIT);
    if limit == 0 || limit > MAX_AUTOMATION_RULE_PREVIEW_LIMIT {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidLimit,
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

/// GET /v1/settings
///
/// @spec docs/L1-api#settings
#[utoipa::path(
    get,
    path = "/v1/settings",
    tag = "settings",
    summary = "Get settings",
    description = "Returns global application settings.",
    responses(
        (status = 200, description = "The application settings", body = AppSettings),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
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
#[utoipa::path(
    patch,
    path = "/v1/settings",
    tag = "settings",
    summary = "Update settings",
    description = "Sparse-merges provided settings fields. Validates that a referenced default \
                   account exists before persisting.",
    request_body = PatchSettingsRequest,
    responses(
        (status = 200, description = "The updated settings", body = AppSettings),
        (status = 400, description = "Validation failed", body = ApiErrorBody)
    )
)]
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
                    ApiErrorCode::InvalidAccount,
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
    if let Some(cache_policy) = &request.cache_policy {
        settings.cache_policy = normalize_cache_policy(cache_policy.clone());
    }
    validate_automation_rules(&settings.automation_rules)?;
    validate_automation_drafts(&settings.automation_rules, &settings.automation_drafts)?;
    let mut changed = Vec::new();
    if request.default_account_id.is_some() {
        changed.push("defaultAccount");
    }
    if request.automation_rules.is_some() {
        changed.push("automationRules");
    }
    if request.automation_drafts.is_some() {
        changed.push("automationDrafts");
    }
    if request.cache_policy.is_some() {
        changed.push("cachePolicy");
    }
    state
        .service
        .put_app_settings(&settings)
        .map_err(ApiError::from_service_error)?;
    append_and_publish_config_event(
        &state,
        EVENT_TOPIC_SETTINGS_UPDATED,
        vec![ResourceChange::app_settings_updated()],
        json!({
            "scope": "app",
            "changed": changed,
        }),
    )
    .map_err(store_error_to_api)?;
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
/// @spec docs/L1-api#application-settings
#[utoipa::path(
    post,
    path = "/v1/automation-rules:preview",
    tag = "settings",
    summary = "Preview automation rule",
    description = "Returns a small newest-first sample and total count for a draft rule condition.",
    request_body = PreviewAutomationRuleRequest,
    responses(
        (status = 200, description = "Preview sample and total count", body = AutomationRulePreviewResponse),
        (status = 400, description = "Invalid limit or condition", body = ApiErrorBody)
    )
)]
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
