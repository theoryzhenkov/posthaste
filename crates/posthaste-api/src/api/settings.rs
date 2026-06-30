use super::*;

/// Request body for `PATCH /v1/settings`.
///
/// @spec docs/L1-api#settings
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchSettingsRequest {
    #[serde(default)]
    pub default_account_id: Option<Option<String>>,
    pub cache_policy: Option<CachePolicy>,
    pub automation_rules: Option<Vec<AutomationRule>>,
    pub automation_drafts: Option<Vec<AutomationRule>>,
    pub appearance: Option<Appearance>,
    pub notifications: Option<Notifications>,
    pub mailbox_colors: Option<Vec<MailboxColor>>,
    /// Per-tag presentation overrides (color + icon); overwrites the stored list.
    pub tags: Option<Vec<TagAppearance>>,
    /// Explicit sidebar arrangement (ids); overwrites the stored list wholesale.
    pub smart_mailbox_order: Option<Vec<SmartMailboxId>>,
    pub account_order: Option<Vec<AccountId>>,
    /// When true, re-run the current backfill rules against existing messages
    /// after persisting (on-demand "backfill now").
    #[serde(default)]
    pub force_backfill: bool,
}

/// Request body for `POST /v1/automation-rules:preview`.
///
/// @spec docs/L1-api#application-settings
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewAutomationRuleRequest {
    pub condition: SmartMailboxRule,
    pub limit: Option<usize>,
}

/// Matching message preview for a draft automation rule condition.
///
/// @spec docs/L1-api#application-settings
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRulePreviewResponse {
    pub total: i64,
    pub items: Vec<MessageSummary>,
}

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
        .runtime
        .get_app_settings(RuntimeCaller::api())
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
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
    state
        .runtime
        .patch_app_settings(
            RuntimeCaller::api(),
            PatchAppSettingsMutation {
                default_account_id: request.default_account_id,
                cache_policy: request.cache_policy,
                automation_rules: request.automation_rules,
                automation_drafts: request.automation_drafts,
                appearance: request.appearance,
                notifications: request.notifications,
                mailbox_colors: request.mailbox_colors,
                tags: request.tags,
                smart_mailbox_order: request.smart_mailbox_order,
                account_order: request.account_order,
                force_backfill: request.force_backfill,
            },
        )
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
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
    state
        .runtime
        .preview_automation_rule(
            RuntimeCaller::api(),
            AutomationRulePreviewMutation {
                condition: request.condition,
                limit,
            },
        )
        .await
        .map(|preview| {
            Json(AutomationRulePreviewResponse {
                total: preview.total,
                items: preview.items,
            })
        })
        .map_err(ApiError::from_runtime_error)
}
