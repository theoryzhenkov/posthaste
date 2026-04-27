use std::convert::Infallible;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::Html;
use axum::response::{IntoResponse, Response};
use axum::Json;
use posthaste_domain::{
    now_iso8601 as domain_now_iso8601, AccountAppearance, AccountConnectionOverview, AccountDriver,
    AccountId, AccountOverview, AccountSettings, AccountTransportSettings, AddToMailboxCommand,
    AppSettings, AutomationAction, AutomationRule, CachePolicy, CachedSenderAddress, CommandResult,
    ConversationCursor, ConversationId, ConversationPage, ConversationSortField,
    ConversationSummary, ConversationView, DomainEvent, EventFilter, GatewayError, Identity,
    ImapTransportSettings, MailboxId, MailboxSummary, MessageAttachment, MessageCursor,
    MessageDetail, MessageId, MessagePage, MessageSortField, MessageSummary, ProviderAuthKind,
    ProviderHint, Recipient, RemoveFromMailboxCommand, ReplaceMailboxesCommand, ReplyContext,
    SecretKind, SecretRef, SecretStatus, SecretStorage, SendMessageRequest, ServiceError,
    SetKeywordsCommand, SharedGateway, SidebarResponse, SmartMailbox, SmartMailboxCondition,
    SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator, SmartMailboxId,
    SmartMailboxKind, SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode,
    SmartMailboxSummary, SmartMailboxValue, SmtpTransportSettings, SortDirection, SyncTrigger,
    TransportSecurity, EVENT_TOPIC_ACCOUNT_CREATED, EVENT_TOPIC_ACCOUNT_DELETED,
    EVENT_TOPIC_ACCOUNT_UPDATED,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::fs;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tracing::warn;

use crate::oauth::{
    OAuthExchangeResult, OAuthProviderProfile, OAuthTokenService, OAuthTokenSet, PendingOAuthFlow,
};
use crate::{sanitize, AppState};

mod account_support;
mod cursor_support;

use account_support::{
    account_overview, account_secret_ref, append_and_publish_account_event, apply_account_patch,
    apply_secret_instruction, default_account_appearance, delete_managed_secret,
    generate_account_id_seed, generate_smart_mailbox_id, internal_error,
    normalize_account_appearance, normalize_automation_rules, normalize_email_patterns,
    normalize_optional, store_error_to_api, validate_account_settings, validate_automation_drafts,
    validate_automation_rules, validate_logo_image_id,
};
use cursor_support::{
    conversation_limit, conversation_page_response, event_to_sse, matches_event, message_limit,
    message_page_response, parse_conversation_cursor, parse_message_cursor,
};

/// Query parameters for conversation list endpoints.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListConversationsQuery {
    pub source_id: Option<String>,
    pub mailbox_id: Option<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub sort: Option<ConversationSortField>,
    pub sort_dir: Option<SortDirection>,
    pub q: Option<String>,
}

/// Query parameters for source-scoped message listing.
///
/// @spec docs/L1-api#conversations-and-messages
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSourceMessagesQuery {
    pub mailbox_id: Option<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub sort: Option<MessageSortField>,
    pub sort_dir: Option<SortDirection>,
    pub q: Option<String>,
}

/// Query parameters for smart-mailbox message listing.
///
/// @spec docs/L1-api#smart-mailboxes
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSmartMailboxMessagesQuery {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub sort: Option<MessageSortField>,
    pub sort_dir: Option<SortDirection>,
    pub q: Option<String>,
}

/// Query parameters for the SSE event stream endpoint.
///
/// @spec docs/L1-api#sse-event-stream
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventsQuery {
    pub account_id: Option<String>,
    pub topic: Option<String>,
    pub mailbox_id: Option<String>,
    pub after_seq: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAttachmentQuery {
    pub download: Option<bool>,
}

fn parse_optional_search_rule(query: Option<&str>) -> Result<Option<SmartMailboxRule>, ApiError> {
    let Some(query) = query else {
        return Ok(None);
    };
    let query = query.trim();
    if query.is_empty() {
        return Ok(None);
    }
    posthaste_domain::search::parse_query(query)
        .map(Some)
        .map_err(|msg| ApiError::new(StatusCode::BAD_REQUEST, "invalid_query", msg))
}

fn rule_condition(field: SmartMailboxField, value: impl Into<String>) -> SmartMailboxRuleNode {
    SmartMailboxRuleNode::Condition(SmartMailboxCondition {
        field,
        operator: SmartMailboxOperator::Equals,
        negated: false,
        value: SmartMailboxValue::String(value.into()),
    })
}

fn all_rule(nodes: Vec<SmartMailboxRuleNode>) -> SmartMailboxRule {
    SmartMailboxRule {
        root: SmartMailboxGroup {
            operator: SmartMailboxGroupOperator::All,
            negated: false,
            nodes,
        },
    }
}

fn combine_rules(rules: Vec<SmartMailboxRule>) -> SmartMailboxRule {
    all_rule(
        rules
            .into_iter()
            .map(|rule| SmartMailboxRuleNode::Group(rule.root))
            .collect(),
    )
}

fn source_message_scope_rule(source_id: &str, mailbox_id: Option<&MailboxId>) -> SmartMailboxRule {
    let mut nodes = vec![rule_condition(SmartMailboxField::SourceId, source_id)];
    if let Some(mailbox_id) = mailbox_id {
        nodes.push(rule_condition(
            SmartMailboxField::MailboxId,
            mailbox_id.as_str(),
        ));
    }
    all_rule(nodes)
}

async fn record_search_cache_visibility(
    state: &Arc<AppState>,
    page: &MessagePage,
    scope_rule: &SmartMailboxRule,
    result_rule: &SmartMailboxRule,
) {
    let total_messages = match state.service.count_messages_by_rule(scope_rule) {
        Ok((_, total)) => total.max(0) as u64,
        Err(error) => {
            warn!(
                error = %error,
                "skipping cache search visibility signals because scope count failed"
            );
            return;
        }
    };
    let result_count = match state.service.count_messages_by_rule(result_rule) {
        Ok((_, total)) => total.max(0) as u64,
        Err(error) => {
            warn!(
                error = %error,
                "skipping cache search visibility signals because result count failed"
            );
            return;
        }
    };
    let account_ids =
        match state
            .service
            .record_cache_search_visibility(page, total_messages, result_count)
        {
            Ok(account_ids) => account_ids,
            Err(error) => {
                warn!(
                    error = %error,
                    "failed to record cache search visibility signals"
                );
                return;
            }
        };
    for account_id in account_ids {
        if let Err(error) = state
            .supervisor
            .trigger_cache_maintenance(&account_id)
            .await
        {
            warn!(
                account_id = %account_id,
                error = %error,
                "failed to trigger cache maintenance after search visibility signal"
            );
        }
    }
}

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

/// Request body for `PATCH /v1/sources/{source_id}/mailboxes/{mailbox_id}`.
///
/// Outer `Option` distinguishes omitted `role` from an explicit JSON `null`.
///
/// @spec docs/L1-api#conversations-and-messages
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchMailboxRequest {
    pub role: Option<Option<String>>,
}

const MAX_ACCOUNT_LOGO_BYTES: usize = 2 * 1024 * 1024;

/// Request body for `PATCH /v1/settings`.
///
/// @spec docs/L1-api#settings
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchSettingsRequest {
    #[serde(default)]
    pub default_account_id: Option<Option<String>>,
    pub cache_policy: Option<CachePolicy>,
    pub automation_rules: Option<Vec<AutomationRule>>,
    pub automation_drafts: Option<Vec<AutomationRule>>,
}

fn normalize_cache_policy(mut policy: CachePolicy) -> CachePolicy {
    policy.hard_cap_bytes = policy.hard_cap_bytes.max(policy.soft_cap_bytes);
    policy
}

/// Request body for `POST /v1/automation-rules:preview`.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewAutomationRuleRequest {
    pub condition: SmartMailboxRule,
    pub limit: Option<usize>,
}

/// Transport fields for account create/patch requests.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountTransportRequest {
    pub provider: Option<ProviderHint>,
    pub auth: Option<ProviderAuthKind>,
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub imap: Option<ImapTransportSettings>,
    pub smtp: Option<SmtpTransportSettings>,
}

/// Tri-state write mode controlling how a secret is mutated on account save.
///
/// @spec docs/L1-api#secret-management
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SecretWriteMode {
    #[default]
    Keep,
    Replace,
    Clear,
}

/// Secret instruction embedded in account create/patch requests.
///
/// @spec docs/L1-api#secret-management
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretWriteRequest {
    #[serde(default)]
    pub mode: SecretWriteMode,
    pub password: Option<String>,
}

/// Request body for `POST /v1/accounts/{account_id}/oauth/start`.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartOAuthRequest {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
}

/// Request body for `POST /v1/oauth/start`.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartProviderOAuthRequest {
    pub provider: ProviderHint,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
}

/// Response body for `POST /v1/accounts/{account_id}/oauth/start`.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartOAuthResponse {
    pub authorization_url: String,
    pub state: String,
    pub redirect_uri: String,
}

/// Query parameters for the loopback OAuth redirect endpoint.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthCallbackQuery {
    pub state: String,
    pub code: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// Request body for `POST /v1/accounts`.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountRequest {
    pub id: Option<String>,
    pub name: String,
    pub full_name: Option<String>,
    #[serde(default)]
    pub email_patterns: Vec<String>,
    pub driver: Option<AccountDriver>,
    pub enabled: Option<bool>,
    pub appearance: Option<AccountAppearance>,
    #[serde(default)]
    pub transport: AccountTransportRequest,
    #[serde(default)]
    pub secret: SecretWriteRequest,
}

/// Request body for `PATCH /v1/accounts/{account_id}`. Omitted fields are preserved.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchAccountRequest {
    pub name: Option<String>,
    pub full_name: Option<String>,
    pub email_patterns: Option<Vec<String>>,
    pub driver: Option<AccountDriver>,
    pub enabled: Option<bool>,
    pub appearance: Option<AccountAppearance>,
    pub transport: Option<AccountTransportRequest>,
    pub secret: Option<SecretWriteRequest>,
}

/// Request body for `POST /v1/smart-mailboxes`.
///
/// @spec docs/L1-api#smart-mailbox-crud
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSmartMailboxRequest {
    pub name: String,
    pub position: Option<i64>,
    pub rule: SmartMailboxRule,
}

/// Request body for `PATCH /v1/smart-mailboxes/{id}`. Omitted fields are preserved.
///
/// @spec docs/L1-api#smart-mailbox-crud
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchSmartMailboxRequest {
    pub name: Option<String>,
    pub position: Option<i64>,
    pub rule: Option<SmartMailboxRule>,
}

/// JSON error response body returned by all API error paths.
///
/// @spec docs/L1-api#error-format
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
}

/// Structured API error carrying an HTTP status code and a JSON body.
///
/// @spec docs/L1-api#error-format
/// @spec docs/L1-api#error-code-mapping
pub struct ApiError {
    status: StatusCode,
    body: ApiErrorBody,
}

/// Generic success response for mutating endpoints that return no domain data.
///
/// @spec docs/L1-api#endpoint-table
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OkResponse {
    pub ok: bool,
}

/// Response from `POST /v1/accounts/{id}/verify`.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResponse {
    pub ok: bool,
    pub identity_email: Option<String>,
    pub push_supported: bool,
}

/// Paginated conversation list response with an opaque cursor for the next page.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPageResponse {
    pub items: Vec<ConversationSummary>,
    pub next_cursor: Option<String>,
}

/// Paginated message list response with an opaque cursor for the next page.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePageResponse {
    pub items: Vec<MessageSummary>,
    pub next_cursor: Option<String>,
}

/// Matching message preview for a draft automation rule condition.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRulePreviewResponse {
    pub total: i64,
    pub items: Vec<MessageSummary>,
}

impl ApiError {
    /// Map a domain `ServiceError` to an HTTP status code and JSON error body.
    ///
    /// @spec docs/L1-api#error-code-mapping
    pub fn from_service_error(error: ServiceError) -> Self {
        let status = match error.code() {
            "not_found" => StatusCode::NOT_FOUND,
            "conflict" | "state_mismatch" => StatusCode::CONFLICT,
            "auth_error" => StatusCode::UNAUTHORIZED,
            "gateway_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
            "network_error" => StatusCode::BAD_GATEWAY,
            "gateway_rejected" | "secret_unavailable" | "secret_unsupported" => {
                StatusCode::BAD_REQUEST
            }
            "config_validation" | "config_parse" => StatusCode::BAD_REQUEST,
            "config_io" => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self {
            status,
            body: ApiErrorBody {
                code: error.code().to_string(),
                message: error.to_string(),
                details: json!({}),
            },
        }
    }

    /// Construct an `ApiError` with explicit status, code, and message.
    pub fn new(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            body: ApiErrorBody {
                code: code.to_string(),
                message: message.into(),
                details: json!({}),
            },
        }
    }
}

impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        Self::from_service_error(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
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
    validate_automation_rules(&settings.automation_rules)?;
    validate_automation_drafts(&settings.automation_rules, &settings.automation_drafts)?;
    state
        .service
        .put_app_settings(&settings)
        .map_err(ApiError::from_service_error)?;
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

/// GET /v1/accounts
///
/// @spec docs/L1-api#accounts
pub async fn list_accounts(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<AccountOverview>>, ApiError> {
    let settings = state
        .service
        .get_app_settings()
        .map_err(ApiError::from_service_error)?;
    let accounts = state
        .service
        .list_sources()
        .map_err(ApiError::from_service_error)?;
    let mut response = Vec::with_capacity(accounts.len());
    for account in accounts {
        response.push(account_overview(&state, &settings, account).await);
    }
    Ok(Json(response))
}

/// GET /v1/accounts/{account_id}
///
/// @spec docs/L1-api#accounts
pub async fn get_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<AccountOverview>, ApiError> {
    let settings = state
        .service
        .get_app_settings()
        .map_err(ApiError::from_service_error)?;
    let account = state
        .service
        .get_source(&AccountId::from(account_id.as_str()))
        .map_err(ApiError::from_service_error)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "account not found"))?;
    Ok(Json(account_overview(&state, &settings, account).await))
}

/// POST /v1/accounts
///
/// Validates uniqueness, applies secret instruction, persists config, starts
/// the supervisor runtime, and emits an `account.created` event.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub async fn create_account(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateAccountRequest>,
) -> Result<Json<AccountOverview>, ApiError> {
    let CreateAccountRequest {
        id,
        name,
        full_name,
        email_patterns,
        driver,
        enabled,
        appearance,
        transport,
        secret,
    } = request;
    let email_patterns = normalize_email_patterns(&email_patterns);
    let account_id = match id {
        Some(id) if !id.trim().is_empty() => AccountId::from(id.trim()),
        _ => {
            let seed = generate_account_id_seed(&name, &email_patterns);
            let mut candidate = AccountId::from(seed.as_str());
            let mut suffix = 2;
            while state
                .service
                .get_source(&candidate)
                .map_err(ApiError::from_service_error)?
                .is_some()
            {
                candidate = AccountId::from(format!("{seed}-{suffix}"));
                suffix += 1;
            }
            candidate
        }
    };
    if state
        .service
        .get_source(&account_id)
        .map_err(ApiError::from_service_error)?
        .is_some()
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "conflict",
            "account already exists",
        ));
    }

    let timestamp = domain_now_iso8601().map_err(internal_error)?;
    let mut account = AccountSettings {
        id: account_id.clone(),
        name: name.trim().to_string(),
        full_name: normalize_optional(full_name),
        email_patterns,
        driver: driver.unwrap_or(AccountDriver::Jmap),
        enabled: enabled.unwrap_or(true),
        appearance: appearance.map(normalize_account_appearance),
        transport: transport.into(),
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };
    apply_secret_instruction(state.as_ref(), &mut account, None, &secret)?;
    validate_account_settings(&account)?;
    state
        .service
        .save_source(&account)
        .map_err(ApiError::from_service_error)?;
    state.supervisor.start_account(&account).await;
    append_and_publish_account_event(&state, &account_id, EVENT_TOPIC_ACCOUNT_CREATED)
        .map_err(store_error_to_api)?;

    let settings = state
        .service
        .get_app_settings()
        .map_err(ApiError::from_service_error)?;
    Ok(Json(account_overview(&state, &settings, account).await))
}

/// PATCH /v1/accounts/{account_id}
///
/// Sparse-merges provided fields into the existing account and restarts
/// the supervisor runtime.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub async fn patch_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
    Json(request): Json<PatchAccountRequest>,
) -> Result<Json<AccountOverview>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let mut account = state
        .service
        .get_source(&account_id)
        .map_err(ApiError::from_service_error)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "account not found"))?;
    let previous_image_id = account_appearance_image_id(&account);
    apply_account_patch(&mut account, &request);
    account.updated_at = domain_now_iso8601().map_err(internal_error)?;
    let existing_secret_ref = account.transport.secret_ref.clone();
    let secret_request = request.secret.unwrap_or_default();
    apply_secret_instruction(
        state.as_ref(),
        &mut account,
        existing_secret_ref.as_ref(),
        &secret_request,
    )?;
    validate_account_settings(&account)?;

    state
        .service
        .save_source(&account)
        .map_err(ApiError::from_service_error)?;
    state.supervisor.start_account(&account).await;
    append_and_publish_account_event(&state, &account_id, EVENT_TOPIC_ACCOUNT_UPDATED)
        .map_err(store_error_to_api)?;
    let next_image_id = account_appearance_image_id(&account);
    if previous_image_id != next_image_id {
        if let Some(previous_image_id) = previous_image_id {
            let _ = delete_account_logo_file(state.as_ref(), &previous_image_id).await;
        }
    }

    let settings = state
        .service
        .get_app_settings()
        .map_err(ApiError::from_service_error)?;
    Ok(Json(account_overview(&state, &settings, account).await))
}

/// POST /v1/accounts/{account_id}/verify
///
/// Attempts JMAP session discovery and reports identity and push support.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub async fn verify_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<VerificationResponse>, ApiError> {
    let account = state
        .service
        .get_source(&AccountId::from(account_id.as_str()))
        .map_err(ApiError::from_service_error)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "account not found"))?;
    let result = state
        .supervisor
        .verify_account(&account)
        .await
        .map_err(ApiError::from_service_error)?;
    Ok(Json(VerificationResponse {
        ok: result.ok,
        identity_email: result.identity.map(|identity| identity.email),
        push_supported: result.push_supported,
    }))
}

/// POST /v1/oauth/start
///
/// Creates a backend-held PKCE authorization session for provider-first setup.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub async fn start_provider_oauth(
    State(state): State<Arc<AppState>>,
    Json(request): Json<StartProviderOAuthRequest>,
) -> Result<Json<StartOAuthResponse>, ApiError> {
    let profile = OAuthProviderProfile::for_provider(&request.provider).ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_provider",
            "provider does not support built-in OAuth",
        )
    })?;
    let (client_id, client_secret, redirect_uri) = validate_oauth_start_request(
        request.client_id.as_str(),
        request.client_secret.as_deref(),
        request.redirect_uri.as_str(),
    )?;

    let oauth = OAuthTokenService::new().map_err(ServiceError::from)?;
    let session = oauth
        .authorization_session(&profile, client_id, client_secret, redirect_uri)
        .map_err(ServiceError::from)?;
    state
        .oauth_flows
        .insert(
            session.state.clone(),
            PendingOAuthFlow {
                account_id: None,
                profile,
                client_id: client_id.to_string(),
                client_secret: client_secret.map(ToString::to_string),
                redirect_uri: redirect_uri.to_string(),
                pkce_verifier: session.pkce_verifier,
                nonce: session.nonce,
            },
        )
        .await;

    Ok(Json(StartOAuthResponse {
        authorization_url: session.authorization_url,
        state: session.state,
        redirect_uri: session.redirect_uri,
    }))
}

/// POST /v1/accounts/{account_id}/oauth/start
///
/// Creates a backend-held PKCE authorization session for an existing account.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub async fn start_account_oauth(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
    Json(request): Json<StartOAuthRequest>,
) -> Result<Json<StartOAuthResponse>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let account = state
        .service
        .get_source(&account_id)
        .map_err(ApiError::from_service_error)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "account not found"))?;
    let profile =
        OAuthProviderProfile::for_provider(&account.transport.provider).ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "account provider does not support built-in OAuth",
            )
        })?;
    let (client_id, client_secret, redirect_uri) = validate_oauth_start_request(
        request.client_id.as_str(),
        request.client_secret.as_deref(),
        request.redirect_uri.as_str(),
    )?;

    let oauth = OAuthTokenService::new().map_err(ServiceError::from)?;
    let session = oauth
        .authorization_session(&profile, client_id, client_secret, redirect_uri)
        .map_err(ServiceError::from)?;
    state
        .oauth_flows
        .insert(
            session.state.clone(),
            PendingOAuthFlow {
                account_id: Some(account_id),
                profile,
                client_id: client_id.to_string(),
                client_secret: client_secret.map(ToString::to_string),
                redirect_uri: redirect_uri.to_string(),
                pkce_verifier: session.pkce_verifier,
                nonce: session.nonce,
            },
        )
        .await;

    Ok(Json(StartOAuthResponse {
        authorization_url: session.authorization_url,
        state: session.state,
        redirect_uri: session.redirect_uri,
    }))
}

/// GET /v1/oauth/callback
///
/// Exchanges a provider authorization code for a token set. Provider-first
/// flows create an account from the OIDC identity; existing-account flows
/// store the token set as the account's managed OS secret.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub async fn complete_account_oauth(
    State(state): State<Arc<AppState>>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Html<String>, ApiError> {
    if let Some(error) = query.error {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "oauth_denied",
            query.error_description.unwrap_or(error),
        ));
    }
    let code = query
        .code
        .as_deref()
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_oauth_callback",
                "OAuth callback is missing code",
            )
        })?;
    let flow = state
        .oauth_flows
        .remove(&query.state)
        .await
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_oauth_callback",
                "OAuth callback state is unknown or already used",
            )
        })?;
    let oauth = OAuthTokenService::new().map_err(ServiceError::from)?;
    let exchange = oauth
        .exchange_authorization_code(
            &flow.profile,
            &flow.client_id,
            flow.client_secret.as_deref(),
            &flow.redirect_uri,
            code,
            &flow.pkce_verifier,
            &flow.nonce,
            time::OffsetDateTime::now_utc(),
        )
        .await
        .map_err(ServiceError::from)?;
    match flow.account_id {
        Some(account_id) => {
            persist_oauth_token_set(&state, &account_id, exchange.token_set).await?;
        }
        None => {
            create_oauth_account_from_exchange(&state, &flow.profile, exchange).await?;
        }
    }

    Ok(Html(
        "<!doctype html><meta charset=\"utf-8\"><title>Posthaste OAuth</title><p>Authentication complete. You can return to Posthaste.</p>".to_string(),
    ))
}

fn validate_oauth_start_request<'a>(
    client_id: &'a str,
    client_secret: Option<&'a str>,
    redirect_uri: &'a str,
) -> Result<(&'a str, Option<&'a str>, &'a str), ApiError> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_oauth_request",
            "clientId is required",
        ));
    }
    let redirect_uri = redirect_uri.trim();
    if redirect_uri.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_oauth_request",
            "redirectUri is required",
        ));
    }
    Ok((
        client_id,
        client_secret
            .map(str::trim)
            .filter(|client_secret| !client_secret.is_empty()),
        redirect_uri,
    ))
}

async fn create_oauth_account_from_exchange(
    state: &Arc<AppState>,
    profile: &OAuthProviderProfile,
    exchange: OAuthExchangeResult,
) -> Result<AccountId, ApiError> {
    let identity_email = exchange.identity_email.trim().to_string();
    let email_patterns = vec![identity_email.clone()];
    let name = identity_email.clone();
    let seed = generate_account_id_seed(&name, &email_patterns);
    let mut account_id = AccountId::from(seed.as_str());
    let mut suffix = 2;
    while state
        .service
        .get_source(&account_id)
        .map_err(ApiError::from_service_error)?
        .is_some()
    {
        account_id = AccountId::from(format!("{seed}-{suffix}"));
        suffix += 1;
    }

    let secret_ref = account_secret_ref(&account_id);
    let timestamp = domain_now_iso8601().map_err(internal_error)?;
    let account = oauth_account_settings(
        account_id.clone(),
        profile.provider.clone(),
        name,
        identity_email,
        email_patterns,
        secret_ref.clone(),
        timestamp,
    )?;
    let encoded = exchange.token_set.encode().map_err(ServiceError::from)?;
    state
        .secret_store
        .save(&secret_ref, &encoded)
        .map_err(ServiceError::from)
        .map_err(ApiError::from)?;

    if let Err(error) = validate_account_settings(&account) {
        delete_managed_secret(state.as_ref(), Some(&secret_ref))?;
        return Err(error);
    }
    if let Err(error) = state.service.save_source(&account) {
        delete_managed_secret(state.as_ref(), Some(&secret_ref))?;
        return Err(ApiError::from_service_error(error));
    }

    state.supervisor.start_account(&account).await;
    append_and_publish_account_event(state, &account_id, EVENT_TOPIC_ACCOUNT_CREATED)
        .map_err(store_error_to_api)?;
    Ok(account_id)
}

fn oauth_account_settings(
    account_id: AccountId,
    provider: ProviderHint,
    name: String,
    identity_email: String,
    email_patterns: Vec<String>,
    secret_ref: SecretRef,
    timestamp: String,
) -> Result<AccountSettings, ApiError> {
    let (imap, smtp) = oauth_provider_mail_transport(&provider)?;
    Ok(AccountSettings {
        id: account_id,
        name,
        full_name: None,
        email_patterns,
        driver: AccountDriver::ImapSmtp,
        enabled: true,
        appearance: None,
        transport: AccountTransportSettings {
            provider,
            auth: ProviderAuthKind::OAuth2,
            base_url: None,
            username: Some(identity_email),
            secret_ref: Some(secret_ref),
            imap: Some(imap),
            smtp: Some(smtp),
        },
        created_at: timestamp.clone(),
        updated_at: timestamp,
    })
}

fn oauth_provider_mail_transport(
    provider: &ProviderHint,
) -> Result<(ImapTransportSettings, SmtpTransportSettings), ApiError> {
    match provider {
        ProviderHint::Gmail => Ok((
            ImapTransportSettings {
                host: "imap.gmail.com".to_string(),
                port: 993,
                security: TransportSecurity::Tls,
            },
            SmtpTransportSettings {
                host: "smtp.gmail.com".to_string(),
                port: 587,
                security: TransportSecurity::StartTls,
            },
        )),
        ProviderHint::Outlook => Ok((
            ImapTransportSettings {
                host: "outlook.office365.com".to_string(),
                port: 993,
                security: TransportSecurity::Tls,
            },
            SmtpTransportSettings {
                host: "smtp.office365.com".to_string(),
                port: 587,
                security: TransportSecurity::StartTls,
            },
        )),
        ProviderHint::Generic | ProviderHint::Icloud => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_provider",
            "provider does not support built-in OAuth account creation",
        )),
    }
}

async fn persist_oauth_token_set(
    state: &Arc<AppState>,
    account_id: &AccountId,
    token_set: OAuthTokenSet,
) -> Result<(), ApiError> {
    let mut account = state
        .service
        .get_source(account_id)
        .map_err(ApiError::from_service_error)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "account not found"))?;
    let previous_secret_ref = account.transport.secret_ref.clone();
    let secret_ref = previous_secret_ref
        .as_ref()
        .filter(|secret_ref| matches!(secret_ref.kind, SecretKind::Os))
        .cloned()
        .unwrap_or_else(|| account_secret_ref(&account.id));
    let encoded = token_set.encode().map_err(ServiceError::from)?;

    match previous_secret_ref.as_ref() {
        Some(existing) if existing == &secret_ref => state
            .secret_store
            .update(&secret_ref, &encoded)
            .map_err(ServiceError::from)
            .map_err(ApiError::from)?,
        _ => {
            delete_managed_secret(state.as_ref(), previous_secret_ref.as_ref())?;
            state
                .secret_store
                .save(&secret_ref, &encoded)
                .map_err(ServiceError::from)
                .map_err(ApiError::from)?;
        }
    }

    account.transport.auth = ProviderAuthKind::OAuth2;
    account.transport.secret_ref = Some(secret_ref);
    account.updated_at = domain_now_iso8601().map_err(internal_error)?;
    validate_account_settings(&account)?;
    state
        .service
        .save_source(&account)
        .map_err(ApiError::from_service_error)?;
    state.supervisor.start_account(&account).await;
    append_and_publish_account_event(state, account_id, EVENT_TOPIC_ACCOUNT_UPDATED)
        .map_err(store_error_to_api)?;

    Ok(())
}

/// POST /v1/accounts/{account_id}/enable
///
/// @spec docs/L1-api#account-crud-lifecycle
pub async fn enable_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    set_account_enabled(state, account_id, true).await
}

/// POST /v1/accounts/{account_id}/disable
///
/// @spec docs/L1-api#account-crud-lifecycle
pub async fn disable_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    set_account_enabled(state, account_id, false).await
}

/// POST /v1/accounts/{account_id}/logo
///
/// Stores a user-uploaded account logo under the config root and updates the
/// account appearance to reference it.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub async fn upload_account_logo(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Json<AccountOverview>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let mut account = state
        .service
        .get_source(&account_id)
        .map_err(ApiError::from_service_error)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "account not found"))?;

    if bytes.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_account_logo",
            "account logo file is empty",
        ));
    }
    if bytes.len() > MAX_ACCOUNT_LOGO_BYTES {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_account_logo",
            "account logo file is too large",
        ));
    }

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .unwrap_or("");
    let extension = account_logo_extension(content_type)?;
    fs::create_dir_all(&state.account_logo_root)
        .await
        .map_err(|err| internal_error(format!("failed to create account logo directory: {err}")))?;
    let image_id = uuid::Uuid::new_v4().simple().to_string();
    let path = state
        .account_logo_root
        .join(format!("{image_id}.{extension}"));
    fs::write(&path, &bytes)
        .await
        .map_err(|err| internal_error(format!("failed to write account logo: {err}")))?;

    let previous_image_id = match &account.appearance {
        Some(AccountAppearance::Image { image_id, .. }) => Some(image_id.clone()),
        _ => None,
    };
    let (initials, color_hue) = account_appearance_fallback_parts(&account);
    account.appearance = Some(AccountAppearance::Image {
        image_id: image_id.clone(),
        initials,
        color_hue,
    });
    account.updated_at = domain_now_iso8601().map_err(internal_error)?;
    validate_account_settings(&account)?;
    if let Err(error) = state.service.save_source(&account) {
        let _ = delete_account_logo_file(state.as_ref(), &image_id).await;
        return Err(ApiError::from_service_error(error));
    }
    append_and_publish_account_event(&state, &account_id, EVENT_TOPIC_ACCOUNT_UPDATED)
        .map_err(store_error_to_api)?;
    if let Some(previous_image_id) = previous_image_id {
        if previous_image_id != image_id {
            let _ = delete_account_logo_file(state.as_ref(), &previous_image_id).await;
        }
    }

    let settings = state
        .service
        .get_app_settings()
        .map_err(ApiError::from_service_error)?;
    Ok(Json(account_overview(&state, &settings, account).await))
}

/// GET /v1/account-assets/logos/{image_id}
///
/// @spec docs/L1-api#account-crud-lifecycle
pub async fn get_account_logo(
    State(state): State<Arc<AppState>>,
    Path(image_id): Path<String>,
) -> Result<Response, ApiError> {
    validate_logo_image_id(&image_id)?;
    for (extension, content_type) in ACCOUNT_LOGO_MIME_TYPES {
        let path = state
            .account_logo_root
            .join(format!("{image_id}.{extension}"));
        if path.exists() {
            let bytes = fs::read(path)
                .await
                .map_err(|err| internal_error(format!("failed to read account logo: {err}")))?;
            let mut response = Response::new(Body::from(bytes));
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=86400"),
            );
            return Ok(response);
        }
    }
    Err(ApiError::new(
        StatusCode::NOT_FOUND,
        "not_found",
        "account logo not found",
    ))
}

/// DELETE /v1/accounts/{account_id}
///
/// Removes the managed OS keyring secret, stops the supervisor runtime,
/// deletes the config file, and emits an `account.deleted` event.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub async fn delete_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let account = state
        .service
        .get_source(&account_id)
        .map_err(ApiError::from_service_error)?;
    let Some(account) = account else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "account not found",
        ));
    };
    let logo_image_id = match &account.appearance {
        Some(AccountAppearance::Image { image_id, .. }) => Some(image_id.clone()),
        _ => None,
    };
    delete_managed_secret(state.as_ref(), account.transport.secret_ref.as_ref())?;
    state.supervisor.remove_account(&account_id).await;
    state
        .service
        .delete_source(&account_id)
        .map_err(ApiError::from_service_error)?;
    append_and_publish_account_event(&state, &account_id, EVENT_TOPIC_ACCOUNT_DELETED)
        .map_err(store_error_to_api)?;
    if let Some(image_id) = logo_image_id {
        let _ = delete_account_logo_file(state.as_ref(), &image_id).await;
    }
    Ok(Json(OkResponse { ok: true }))
}

/// POST /v1/config:reload
///
/// Re-reads config from disk, diffs against the in-memory snapshot, and
/// starts/stops supervisor runtimes for changed accounts.
///
/// @spec docs/L1-api#sync-and-events
/// @spec docs/L1-accounts#configdiff
pub async fn reload_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<OkResponse>, ApiError> {
    let diff = state
        .service
        .reload_config()
        .map_err(ApiError::from_service_error)?;

    // Apply diff to supervisor
    for id in &diff.removed_sources {
        state.supervisor.remove_account(id).await;
    }
    for id in diff.added_sources.iter().chain(diff.changed_sources.iter()) {
        let source = state
            .service
            .get_source(id)
            .map_err(ApiError::from_service_error)?;
        if let Some(source) = source {
            state.supervisor.start_account(&source).await;
        }
    }

    Ok(Json(OkResponse { ok: true }))
}

/// GET /v1/sources/{source_id}/mailboxes
///
/// @spec docs/L1-api#conversations-and-messages
pub async fn list_mailboxes(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
) -> Result<Json<Vec<MailboxSummary>>, ApiError> {
    state
        .service
        .list_mailboxes(&AccountId(source_id))
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// PATCH /v1/sources/{source_id}/mailboxes/{mailbox_id}
///
/// @spec docs/L1-api#conversations-and-messages
/// @spec docs/L1-jmap#methods-used
pub async fn patch_mailbox(
    State(state): State<Arc<AppState>>,
    Path((source_id, mailbox_id)): Path<(String, String)>,
    Json(request): Json<PatchMailboxRequest>,
) -> Result<Json<Vec<MailboxSummary>>, ApiError> {
    let role = validate_patch_mailbox_role(request.role)?;
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    let events = state
        .service
        .set_mailbox_role(
            &account_id,
            &MailboxId(mailbox_id),
            role.as_deref(),
            gateway.as_ref(),
        )
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&events);
    state
        .service
        .list_mailboxes(&account_id)
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// GET /v1/sources/{source_id}/messages
///
/// @spec docs/L1-api#conversations-and-messages
pub async fn list_source_messages(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    Query(query): Query<ListSourceMessagesQuery>,
) -> Result<Json<MessagePageResponse>, ApiError> {
    let mailbox_id = query.mailbox_id.map(MailboxId);
    let limit = message_limit(query.limit)?;
    let cursor = parse_message_cursor(query.cursor.as_deref())?;
    let sort_field = query.sort.unwrap_or_default();
    let sort_direction = query.sort_dir.unwrap_or_default();
    if let Some(search_rule) = parse_optional_search_rule(query.q.as_deref())? {
        let scoped_rule = source_message_scope_rule(&source_id, mailbox_id.as_ref());
        let result_rule = combine_rules(vec![scoped_rule.clone(), search_rule]);
        let page = state
            .service
            .query_message_page_by_rule(
                &result_rule,
                limit,
                cursor.as_ref(),
                sort_field,
                sort_direction,
            )
            .map_err(ApiError::from_service_error)?;
        record_search_cache_visibility(&state, &page, &scoped_rule, &result_rule).await;
        return Ok(Json(message_page_response(page)));
    }
    let page = state
        .service
        .list_message_page(
            &AccountId(source_id),
            mailbox_id.as_ref(),
            limit,
            cursor.as_ref(),
            sort_field,
            sort_direction,
        )
        .map_err(ApiError::from_service_error)?;
    Ok(Json(message_page_response(page)))
}

/// GET /v1/sidebar
///
/// @spec docs/L1-api#navigation
pub async fn get_sidebar(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SidebarResponse>, ApiError> {
    state
        .service
        .get_sidebar()
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// GET /v1/smart-mailboxes
///
/// @spec docs/L1-api#smart-mailboxes
pub async fn list_smart_mailboxes(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SmartMailboxSummary>>, ApiError> {
    state
        .service
        .list_smart_mailboxes()
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// POST /v1/smart-mailboxes
///
/// Generates an ID from the name (`sm-{slug}-{uuid}`) and persists to config.
///
/// @spec docs/L1-api#smart-mailbox-crud
pub async fn create_smart_mailbox(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateSmartMailboxRequest>,
) -> Result<Json<SmartMailbox>, ApiError> {
    let timestamp = domain_now_iso8601().map_err(internal_error)?;
    let smart_mailbox = SmartMailbox {
        id: SmartMailboxId::from(generate_smart_mailbox_id(&request.name)),
        name: request.name,
        position: request.position.unwrap_or(0),
        kind: SmartMailboxKind::User,
        default_key: None,
        parent_id: None,
        rule: request.rule,
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };
    state
        .service
        .save_smart_mailbox(&smart_mailbox)
        .map_err(ApiError::from_service_error)?;
    Ok(Json(smart_mailbox))
}

/// GET /v1/smart-mailboxes/{id}
///
/// @spec docs/L1-api#smart-mailboxes
pub async fn get_smart_mailbox(
    State(state): State<Arc<AppState>>,
    Path(smart_mailbox_id): Path<String>,
) -> Result<Json<SmartMailbox>, ApiError> {
    state
        .service
        .get_smart_mailbox(&SmartMailboxId::from(smart_mailbox_id))
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// PATCH /v1/smart-mailboxes/{id}
///
/// Merges name, position, and rule fields. Omitted fields are preserved.
///
/// @spec docs/L1-api#smart-mailbox-crud
pub async fn patch_smart_mailbox(
    State(state): State<Arc<AppState>>,
    Path(smart_mailbox_id): Path<String>,
    Json(request): Json<PatchSmartMailboxRequest>,
) -> Result<Json<SmartMailbox>, ApiError> {
    let smart_mailbox_id = SmartMailboxId::from(smart_mailbox_id);
    let mut smart_mailbox = state
        .service
        .get_smart_mailbox(&smart_mailbox_id)
        .map_err(ApiError::from_service_error)?;
    if let Some(name) = request.name {
        smart_mailbox.name = name;
    }
    if let Some(position) = request.position {
        smart_mailbox.position = position;
    }
    if let Some(rule) = request.rule {
        smart_mailbox.rule = rule;
    }
    smart_mailbox.updated_at = domain_now_iso8601().map_err(internal_error)?;
    state
        .service
        .save_smart_mailbox(&smart_mailbox)
        .map_err(ApiError::from_service_error)?;
    Ok(Json(smart_mailbox))
}

/// DELETE /v1/smart-mailboxes/{id}
///
/// @spec docs/L1-api#smart-mailboxes
pub async fn delete_smart_mailbox(
    State(state): State<Arc<AppState>>,
    Path(smart_mailbox_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    state
        .service
        .delete_smart_mailbox(&SmartMailboxId::from(smart_mailbox_id))
        .map_err(ApiError::from_service_error)?;
    Ok(Json(OkResponse { ok: true }))
}

/// POST /v1/smart-mailboxes:reset-defaults
///
/// Restores default smart mailboxes (Inbox, Archive, Drafts, Sent, Junk,
/// Trash, All Mail) and returns the full list.
///
/// @spec docs/L1-api#smart-mailbox-crud
/// @spec docs/L1-accounts#smart-mailbox-defaults
pub async fn reset_default_smart_mailboxes(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SmartMailboxSummary>>, ApiError> {
    state
        .service
        .reset_default_smart_mailboxes()
        .map_err(ApiError::from_service_error)?;
    state
        .service
        .list_smart_mailboxes()
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// GET /v1/smart-mailboxes/{id}/messages
///
/// @spec docs/L1-api#smart-mailboxes
pub async fn list_smart_mailbox_messages(
    State(state): State<Arc<AppState>>,
    Path(smart_mailbox_id): Path<String>,
    Query(query): Query<ListSmartMailboxMessagesQuery>,
) -> Result<Json<MessagePageResponse>, ApiError> {
    let limit = message_limit(query.limit)?;
    let cursor = parse_message_cursor(query.cursor.as_deref())?;
    let sort_field = query.sort.unwrap_or_default();
    let sort_direction = query.sort_dir.unwrap_or_default();
    let smart_mailbox_id = SmartMailboxId::from(smart_mailbox_id);
    if let Some(search_rule) = parse_optional_search_rule(query.q.as_deref())? {
        let mailbox = state
            .service
            .get_smart_mailbox(&smart_mailbox_id)
            .map_err(ApiError::from_service_error)?;
        let scope_rule = mailbox.rule;
        let result_rule = combine_rules(vec![scope_rule.clone(), search_rule]);
        let page = state
            .service
            .query_message_page_by_rule(
                &result_rule,
                limit,
                cursor.as_ref(),
                sort_field,
                sort_direction,
            )
            .map_err(ApiError::from_service_error)?;
        record_search_cache_visibility(&state, &page, &scope_rule, &result_rule).await;
        return Ok(Json(message_page_response(page)));
    }

    let page = state
        .service
        .list_smart_mailbox_message_page(
            &smart_mailbox_id,
            limit,
            cursor.as_ref(),
            sort_field,
            sort_direction,
        )
        .map_err(ApiError::from_service_error)?;
    Ok(Json(message_page_response(page)))
}

/// GET /v1/smart-mailboxes/{id}/conversations
///
/// @spec docs/L1-api#smart-mailboxes
/// @spec docs/L1-api#cursor-pagination
pub async fn list_smart_mailbox_conversations(
    State(state): State<Arc<AppState>>,
    Path(smart_mailbox_id): Path<String>,
    Query(query): Query<ListConversationsQuery>,
) -> Result<Json<ConversationPageResponse>, ApiError> {
    let limit = conversation_limit(query.limit)?;
    let cursor = parse_conversation_cursor(query.cursor.as_deref())?;
    let sort_field = query.sort.unwrap_or_default();
    let sort_direction = query.sort_dir.unwrap_or_default();

    // When a search query is provided, AND it with the smart mailbox rule.
    if let Some(q) = &query.q {
        if !q.trim().is_empty() {
            let search_rule = parse_optional_search_rule(Some(q))?.expect("non-empty query");
            let mailbox = state
                .service
                .get_smart_mailbox(&SmartMailboxId::from(smart_mailbox_id))
                .map_err(ApiError::from_service_error)?;
            let combined = combine_rules(vec![mailbox.rule, search_rule]);
            return state
                .service
                .query_conversations_by_rule(
                    &combined,
                    limit,
                    cursor.as_ref(),
                    sort_field,
                    sort_direction,
                )
                .map(conversation_page_response)
                .map(Json)
                .map_err(ApiError::from_service_error);
        }
    }

    state
        .service
        .list_smart_mailbox_conversations(
            &SmartMailboxId::from(smart_mailbox_id),
            limit,
            cursor.as_ref(),
            sort_field,
            sort_direction,
        )
        .map(conversation_page_response)
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// GET /v1/views/conversations
///
/// @spec docs/L1-api#conversations-and-messages
/// @spec docs/L1-api#cursor-pagination
pub async fn list_conversations(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListConversationsQuery>,
) -> Result<Json<ConversationPageResponse>, ApiError> {
    let limit = conversation_limit(query.limit)?;
    let cursor = parse_conversation_cursor(query.cursor.as_deref())?;
    let sort_field = query.sort.unwrap_or_default();
    let sort_direction = query.sort_dir.unwrap_or_default();

    // When a search query is provided, parse it into a rule and search globally.
    if let Some(q) = &query.q {
        if !q.trim().is_empty() {
            let rule = parse_optional_search_rule(Some(q))?.expect("non-empty query");
            return state
                .service
                .query_conversations_by_rule(
                    &rule,
                    limit,
                    cursor.as_ref(),
                    sort_field,
                    sort_direction,
                )
                .map(conversation_page_response)
                .map(Json)
                .map_err(ApiError::from_service_error);
        }
    }

    let source_id = query.source_id.as_deref().map(AccountId::from);
    let mailbox_id = query.mailbox_id.as_deref().map(MailboxId::from);
    state
        .service
        .list_conversations(
            source_id.as_ref(),
            mailbox_id.as_ref(),
            limit,
            cursor.as_ref(),
            sort_field,
            sort_direction,
        )
        .map(conversation_page_response)
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// GET /v1/views/conversations/{id}
///
/// @spec docs/L1-api#conversations-and-messages
pub async fn get_conversation(
    State(state): State<Arc<AppState>>,
    Path(conversation_id): Path<String>,
) -> Result<Json<ConversationView>, ApiError> {
    state
        .service
        .get_conversation(&ConversationId::from(conversation_id))
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// GET /v1/sources/{source_id}/messages/{id}
///
/// Sanitizes `body_html` through [`sanitize::sanitize_email_html`] before
/// returning to the frontend.
///
/// @spec docs/L1-api#conversations-and-messages
/// @spec docs/L1-api#message-body-sanitization
pub async fn get_message(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
) -> Result<Json<MessageDetail>, ApiError> {
    let account_id = AccountId(source_id.clone());
    let message_id_ref = MessageId(message_id.clone());
    let gateway = optional_live_gateway(state.as_ref(), &account_id).await;
    let result = state
        .service
        .get_message_detail(&account_id, &message_id_ref, gateway.as_deref())
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&result.events);
    let mut detail = result.detail.ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "message detail not available",
        )
    })?;
    detail.body_html = detail
        .body_html
        .as_ref()
        .map(|html| sanitize::sanitize_email_html(html))
        .map(|html| {
            rewrite_inline_attachment_urls(&html, &source_id, &message_id, &detail.attachments)
        });
    Ok(Json(detail))
}

/// GET /v1/sources/{source_id}/messages/{message_id}/attachments/{attachment_id}
pub async fn get_message_attachment(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id, attachment_id)): Path<(String, String, String)>,
    Query(query): Query<GetAttachmentQuery>,
) -> Result<Response, ApiError> {
    let account_id = AccountId(source_id);
    let message_id = MessageId(message_id);
    let gateway = optional_live_gateway(state.as_ref(), &account_id).await;
    let result = state
        .service
        .get_message_detail(&account_id, &message_id, gateway.as_deref())
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&result.events);
    let detail = result.detail.ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "message detail not available",
        )
    })?;
    let attachment = detail
        .attachments
        .into_iter()
        .find(|attachment| attachment.id == attachment_id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "attachment not found"))?;
    let gateway = require_live_gateway(gateway, &account_id)?;
    let bytes = state
        .service
        .download_blob(&account_id, &attachment.blob_id, gateway.as_ref())
        .await
        .map_err(ApiError::from_service_error)?;

    let disposition_kind = if query.download.unwrap_or(false) {
        "attachment"
    } else {
        "inline"
    };
    let filename = attachment.filename.as_deref().unwrap_or("attachment");
    let content_disposition = format!(
        "{disposition_kind}; filename=\"{}\"",
        escape_content_disposition_filename(filename)
    );

    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(attachment.mime_type.as_str())
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition)
            .map_err(|_| internal_error("invalid content disposition header".to_string()))?,
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=300"),
    );
    Ok(response)
}

/// GET /v1/sources/{source_id}/identity
///
/// @spec docs/L1-api#compose
pub async fn get_identity(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
) -> Result<Json<Identity>, ApiError> {
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    state
        .service
        .fetch_identity(&account_id, gateway.as_ref())
        .await
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// GET /v1/sender-addresses
///
/// @spec docs/L1-api#compose
pub async fn list_sender_addresses(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<CachedSenderAddress>>, ApiError> {
    state
        .store
        .list_sender_address_cache()
        .map(Json)
        .map_err(store_error_to_api)
}

/// GET /v1/sources/{source_id}/messages/{id}/reply-context
///
/// @spec docs/L1-api#compose
/// @spec docs/L1-compose#reply-quoting
pub async fn get_reply_context(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
) -> Result<Json<ReplyContext>, ApiError> {
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    state
        .service
        .fetch_reply_context(&account_id, &MessageId(message_id), gateway.as_ref())
        .await
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// POST /v1/sources/{source_id}/commands/send
///
/// @spec docs/L1-api#compose
/// @spec docs/L1-compose#no-send-empty-to
pub async fn send_message(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    Json(request): Json<SendMessageRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    validate_send_message_request(&request)?;
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    state
        .service
        .send_message(&account_id, &request, gateway.as_ref())
        .await
        .map_err(ApiError::from_service_error)?;
    if let Some(sender) = &request.from {
        if let Err(error) = state.store.remember_sender_address(&account_id, sender) {
            warn!(
                source_id = %account_id,
                sender = %sender.email,
                error = %error,
                "send accepted but sender address cache update failed"
            );
        }
    }
    if let Err(error) = state
        .supervisor
        .trigger_account_sync(&account_id, SyncTrigger::Manual)
        .await
    {
        warn!(
            source_id = %account_id,
            error = %error,
            "send accepted but follow-up sync trigger failed"
        );
    }
    Ok(Json(OkResponse { ok: true }))
}

fn rewrite_inline_attachment_urls(
    html: &str,
    source_id: &str,
    message_id: &str,
    attachments: &[MessageAttachment],
) -> String {
    let mut rewritten = html.to_string();
    for attachment in attachments {
        if !attachment.is_inline {
            continue;
        }
        let Some(cid) = attachment.cid.as_deref() else {
            continue;
        };
        let normalized = cid.trim().trim_start_matches('<').trim_end_matches('>');
        let url = format!(
            "/v1/sources/{source_id}/messages/{message_id}/attachments/{}",
            attachment.id
        );
        rewritten = rewritten.replace(&format!("cid:{normalized}"), &url);
        rewritten = rewritten.replace(&format!("cid:<{normalized}>"), &url);
    }
    rewritten
}

fn escape_content_disposition_filename(filename: &str) -> String {
    filename.replace('\\', "_").replace('"', "'")
}

fn validate_patch_mailbox_role(role: Option<Option<String>>) -> Result<Option<String>, ApiError> {
    let Some(role) = role else {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_mailbox",
            "role is required",
        ));
    };
    match role.as_deref() {
        None | Some("archive") | Some("drafts") | Some("inbox") | Some("junk") | Some("sent")
        | Some("trash") => Ok(role),
        Some(_) => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_mailbox",
            "unsupported mailbox role",
        )),
    }
}

fn validate_send_message_request(request: &SendMessageRequest) -> Result<(), ApiError> {
    if request.from.as_ref().is_some_and(recipient_email_is_empty) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_compose",
            "sender email address cannot be empty",
        ));
    }
    if request.to.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_compose",
            "at least one To recipient is required",
        ));
    }
    if request.subject.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_compose",
            "subject is required",
        ));
    }
    if request.body.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_compose",
            "message body is required",
        ));
    }
    if request
        .to
        .iter()
        .chain(request.cc.iter())
        .chain(request.bcc.iter())
        .any(recipient_email_is_empty)
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_compose",
            "recipient email addresses cannot be empty",
        ));
    }
    Ok(())
}

fn recipient_email_is_empty(recipient: &Recipient) -> bool {
    recipient.email.trim().is_empty()
}

async fn live_gateway(state: &AppState, account_id: &AccountId) -> Result<SharedGateway, ApiError> {
    state
        .supervisor
        .gateway(account_id)
        .await
        .map_err(ApiError::from_service_error)
}

async fn optional_live_gateway(state: &AppState, account_id: &AccountId) -> Option<SharedGateway> {
    state.supervisor.gateway(account_id).await.ok()
}

fn require_live_gateway(
    gateway: Option<SharedGateway>,
    account_id: &AccountId,
) -> Result<SharedGateway, ApiError> {
    gateway.ok_or_else(|| {
        ApiError::from_service_error(ServiceError::from(GatewayError::Unavailable(
            account_id.to_string(),
        )))
    })
}

/// POST /v1/sources/{sid}/commands/messages/{mid}/set-keywords
///
/// @spec docs/L1-api#message-commands
pub async fn set_keywords(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
    Json(command): Json<SetKeywordsCommand>,
) -> Result<Json<CommandResult>, ApiError> {
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    let result = state
        .service
        .set_keywords(
            &account_id,
            &MessageId(message_id),
            &command,
            gateway.as_ref(),
        )
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&result.events);
    Ok(Json(result))
}

/// POST /v1/sources/{sid}/commands/messages/{mid}/add-to-mailbox
///
/// @spec docs/L1-api#message-commands
pub async fn add_to_mailbox(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
    Json(command): Json<AddToMailboxCommand>,
) -> Result<Json<CommandResult>, ApiError> {
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    let result = state
        .service
        .add_to_mailbox(
            &account_id,
            &MessageId(message_id),
            &command,
            gateway.as_ref(),
        )
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&result.events);
    Ok(Json(result))
}

/// POST /v1/sources/{sid}/commands/messages/{mid}/remove-from-mailbox
///
/// @spec docs/L1-api#message-commands
pub async fn remove_from_mailbox(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
    Json(command): Json<RemoveFromMailboxCommand>,
) -> Result<Json<CommandResult>, ApiError> {
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    let result = state
        .service
        .remove_from_mailbox(
            &account_id,
            &MessageId(message_id),
            &command,
            gateway.as_ref(),
        )
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&result.events);
    Ok(Json(result))
}

/// POST /v1/sources/{sid}/commands/messages/{mid}/replace-mailboxes
///
/// @spec docs/L1-api#message-commands
pub async fn replace_mailboxes(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
    Json(command): Json<ReplaceMailboxesCommand>,
) -> Result<Json<CommandResult>, ApiError> {
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    let result = state
        .service
        .replace_mailboxes(
            &account_id,
            &MessageId(message_id),
            &command,
            gateway.as_ref(),
        )
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&result.events);
    Ok(Json(result))
}

/// POST /v1/sources/{sid}/commands/messages/{mid}/destroy
///
/// @spec docs/L1-api#message-commands
pub async fn destroy_message(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
) -> Result<Json<CommandResult>, ApiError> {
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    let result = state
        .service
        .destroy_message(&account_id, &MessageId(message_id), gateway.as_ref())
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&result.events);
    Ok(Json(result))
}

/// POST /v1/sources/{source_id}/commands/sync
///
/// @spec docs/L1-api#sync-and-events
/// @spec docs/L1-sync#sync-loop
pub async fn trigger_sync(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let account_id = AccountId(source_id);
    let event_count = state
        .supervisor
        .sync_account(&account_id)
        .await
        .map_err(ApiError::from_service_error)?;
    Ok(Json(json!({ "ok": true, "eventCount": event_count })))
}

/// GET /v1/events
///
/// Opens an SSE stream. When `afterSeq` is provided, replays matching events
/// from the backlog before switching to the live broadcast stream.
///
/// @spec docs/L1-api#sse-event-stream
/// @spec docs/L0-api#server-sent-events-for-push
pub async fn stream_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let filter = EventFilter {
        account_id: query.account_id.map(AccountId),
        topic: query.topic,
        mailbox_id: query.mailbox_id.map(MailboxId),
        after_seq: query.after_seq,
    };
    let receiver = state.event_sender.subscribe();
    let backlog = if filter.after_seq.is_some() {
        state
            .service
            .list_events(&filter)
            .map_err(ApiError::from_service_error)?
    } else {
        Vec::new()
    };
    let replayed_through = backlog.last().map(|event| event.seq).or(filter.after_seq);
    let backlog_filter = filter.clone();
    let backlog_stream = tokio_stream::iter(
        backlog
            .into_iter()
            .filter(move |event| matches_event(event, &backlog_filter))
            .map(event_to_sse),
    );
    let live_filter = filter.clone();
    let live_stream = BroadcastStream::new(receiver).filter_map(move |message| {
        let live_filter = live_filter.clone();
        match message {
            Ok(event)
                if is_live_event_after_backlog(&event, replayed_through)
                    && matches_event(&event, &live_filter) =>
            {
                Some(event_to_sse(event))
            }
            _ => None,
        }
    });
    Ok(Sse::new(backlog_stream.chain(live_stream)).keep_alive(KeepAlive::default()))
}

fn is_live_event_after_backlog(event: &DomainEvent, replayed_through: Option<i64>) -> bool {
    replayed_through.is_none_or(|seq| event.seq > seq)
}

/// Toggle the `enabled` flag on an account, re-persist, and restart the supervisor.
///
/// @spec docs/L1-api#account-crud-lifecycle
async fn set_account_enabled(
    state: Arc<AppState>,
    account_id: String,
    enabled: bool,
) -> Result<Json<OkResponse>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let mut account = state
        .service
        .get_source(&account_id)
        .map_err(ApiError::from_service_error)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "account not found"))?;
    account.enabled = enabled;
    account.updated_at = domain_now_iso8601().map_err(internal_error)?;
    state
        .service
        .save_source(&account)
        .map_err(ApiError::from_service_error)?;
    state.supervisor.start_account(&account).await;
    append_and_publish_account_event(&state, &account_id, EVENT_TOPIC_ACCOUNT_UPDATED)
        .map_err(store_error_to_api)?;
    Ok(Json(OkResponse { ok: true }))
}

const ACCOUNT_LOGO_MIME_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("webp", "image/webp"),
    ("gif", "image/gif"),
];

fn account_logo_extension(content_type: &str) -> Result<&'static str, ApiError> {
    match content_type {
        "image/png" => Ok("png"),
        "image/jpeg" => Ok("jpg"),
        "image/webp" => Ok("webp"),
        "image/gif" => Ok("gif"),
        _ => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_account_logo",
            "account logo must be a PNG, JPEG, WebP, or GIF image",
        )),
    }
}

fn account_appearance_fallback_parts(account: &AccountSettings) -> (String, u16) {
    let appearance = account
        .appearance
        .clone()
        .unwrap_or_else(|| default_account_appearance(account));
    match normalize_account_appearance(appearance) {
        AccountAppearance::Initials {
            initials,
            color_hue,
        } => (initials, color_hue),
        AccountAppearance::Image {
            initials,
            color_hue,
            ..
        } => (initials, color_hue),
    }
}

fn account_appearance_image_id(account: &AccountSettings) -> Option<String> {
    match &account.appearance {
        Some(AccountAppearance::Image { image_id, .. }) => Some(image_id.clone()),
        _ => None,
    }
}

async fn delete_account_logo_file(state: &AppState, image_id: &str) -> Result<(), ApiError> {
    validate_logo_image_id(image_id)?;
    for (extension, _) in ACCOUNT_LOGO_MIME_TYPES {
        let path = state
            .account_logo_root
            .join(format!("{image_id}.{extension}"));
        match fs::remove_file(path).await {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(internal_error(format!(
                    "failed to delete previous account logo: {error}"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use account_support::{secret_status, validate_secret_request};
    use cursor_support::{encode_conversation_cursor, encode_message_cursor};
    use posthaste_domain::{GatewayError, EVENT_TOPIC_MESSAGE_ARRIVED};

    #[test]
    fn conversation_cursor_round_trips() {
        let cursor = ConversationCursor {
            sort_value: "2026-04-01T10:11:12Z".to_string(),
            conversation_id: ConversationId::from("conv-42"),
        };

        let encoded = encode_conversation_cursor(&cursor);
        let decoded = parse_conversation_cursor(Some(&encoded))
            .unwrap_or_else(|_| panic!("cursor should parse"))
            .unwrap_or_else(|| panic!("cursor should be present"));

        assert_eq!(decoded.sort_value, cursor.sort_value);
        assert_eq!(decoded.conversation_id, cursor.conversation_id);
    }

    #[test]
    fn malformed_conversation_cursor_is_rejected() {
        let error = parse_conversation_cursor(Some("broken-cursor")).unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.body.code, "invalid_cursor");
    }

    #[test]
    fn message_cursor_round_trips() {
        let cursor = MessageCursor {
            sort_value: "2026-04-01T10:11:12Z".to_string(),
            source_id: AccountId::from("primary"),
            message_id: MessageId::from("message-42"),
        };

        let encoded = encode_message_cursor(&cursor);
        let decoded = parse_message_cursor(Some(&encoded))
            .unwrap_or_else(|_| panic!("cursor should parse"))
            .unwrap_or_else(|| panic!("cursor should be present"));

        assert_eq!(decoded.sort_value, cursor.sort_value);
        assert_eq!(decoded.source_id, cursor.source_id);
        assert_eq!(decoded.message_id, cursor.message_id);
    }

    #[test]
    fn message_cursor_allows_empty_sort_value() {
        let cursor = MessageCursor {
            sort_value: String::new(),
            source_id: AccountId::from("primary"),
            message_id: MessageId::from("message-42"),
        };

        let encoded = encode_message_cursor(&cursor);
        let decoded = parse_message_cursor(Some(&encoded))
            .unwrap_or_else(|_| panic!("cursor should parse"))
            .unwrap_or_else(|| panic!("cursor should be present"));

        assert_eq!(decoded.sort_value, cursor.sort_value);
        assert_eq!(decoded.source_id, cursor.source_id);
        assert_eq!(decoded.message_id, cursor.message_id);
    }

    #[test]
    fn malformed_message_cursor_is_rejected() {
        let error = parse_message_cursor(Some("broken-cursor")).unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.body.code, "invalid_cursor");
    }

    #[test]
    fn invalid_search_query_is_rejected() {
        let error = parse_optional_search_rule(Some("wat:nope")).unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.body.code, "invalid_query");
    }

    #[test]
    fn source_scope_rule_combines_source_and_mailbox() {
        let rule = source_message_scope_rule("primary", Some(&MailboxId::from("inbox")));

        assert_eq!(rule.root.operator, SmartMailboxGroupOperator::All);
        assert_eq!(rule.root.nodes.len(), 2);
    }

    #[test]
    fn matches_event_applies_all_filters() {
        let event = DomainEvent {
            seq: 5,
            account_id: AccountId::from("primary"),
            topic: EVENT_TOPIC_MESSAGE_ARRIVED.to_string(),
            occurred_at: "2026-03-31T10:00:00Z".to_string(),
            mailbox_id: Some(MailboxId::from("inbox")),
            message_id: Some(MessageId::from("message-1")),
            payload: json!({"messageId": "message-1"}),
        };
        let matching_filter = EventFilter {
            account_id: Some(AccountId::from("primary")),
            topic: Some(EVENT_TOPIC_MESSAGE_ARRIVED.to_string()),
            mailbox_id: Some(MailboxId::from("inbox")),
            after_seq: Some(4),
        };
        assert!(matches_event(&event, &matching_filter));
        assert!(matches_event(
            &event,
            &EventFilter {
                account_id: None,
                topic: Some(EVENT_TOPIC_MESSAGE_ARRIVED.to_string()),
                mailbox_id: Some(MailboxId::from("inbox")),
                after_seq: Some(4),
            }
        ));
        assert!(!matches_event(
            &event,
            &EventFilter {
                account_id: Some(AccountId::from("secondary")),
                topic: Some(EVENT_TOPIC_MESSAGE_ARRIVED.to_string()),
                mailbox_id: Some(MailboxId::from("inbox")),
                after_seq: Some(4),
            }
        ));
    }

    #[test]
    fn live_events_skip_sequences_already_replayed_from_backlog() {
        let event = DomainEvent {
            seq: 9,
            account_id: AccountId::from("primary"),
            topic: EVENT_TOPIC_MESSAGE_ARRIVED.to_string(),
            occurred_at: "2026-03-31T10:00:00Z".to_string(),
            mailbox_id: None,
            message_id: None,
            payload: json!({}),
        };

        assert!(!is_live_event_after_backlog(&event, Some(9)));
        assert!(!is_live_event_after_backlog(&event, Some(10)));
        assert!(is_live_event_after_backlog(&event, Some(8)));
        assert!(is_live_event_after_backlog(&event, None));
    }

    #[test]
    fn api_error_maps_state_mismatch_to_conflict() {
        let error = ApiError::from_service_error(ServiceError::from(GatewayError::StateMismatch));

        assert_eq!(error.status, StatusCode::CONFLICT);
        assert_eq!(error.body.code, "state_mismatch");
    }

    #[test]
    fn send_message_rejects_missing_to_recipient() {
        let error = validate_send_message_request(&SendMessageRequest {
            from: None,
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Hello".to_string(),
            body: "Body".to_string(),
            in_reply_to: None,
            references: None,
        })
        .expect_err("empty To should be rejected");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.body.code, "invalid_compose");
    }

    #[test]
    fn jmap_account_requires_configured_secret() {
        let account = AccountSettings {
            id: AccountId::from("primary"),
            name: "Primary".to_string(),
            full_name: None,
            email_patterns: Vec::new(),
            driver: AccountDriver::Jmap,
            enabled: true,
            appearance: None,
            transport: posthaste_domain::AccountTransportSettings {
                base_url: Some("https://example.com/jmap".to_string()),
                username: Some("alice@example.com".to_string()),
                secret_ref: None,
                ..Default::default()
            },
            created_at: "2026-03-31T10:00:00Z".to_string(),
            updated_at: "2026-03-31T10:00:00Z".to_string(),
        };

        let error = validate_account_settings(&account).expect_err("validation should fail");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.body.code, "invalid_account");
    }

    #[test]
    fn jmap_account_allows_bearer_secret_without_username() {
        let account = AccountSettings {
            id: AccountId::from("primary"),
            name: "Primary".to_string(),
            full_name: None,
            email_patterns: Vec::new(),
            driver: AccountDriver::Jmap,
            enabled: true,
            appearance: None,
            transport: posthaste_domain::AccountTransportSettings {
                base_url: Some("https://example.com/jmap".to_string()),
                username: None,
                secret_ref: Some(SecretRef {
                    kind: SecretKind::Env,
                    key: "POSTHASTE_JMAP_TOKEN".to_string(),
                }),
                ..Default::default()
            },
            created_at: "2026-03-31T10:00:00Z".to_string(),
            updated_at: "2026-03-31T10:00:00Z".to_string(),
        };

        assert!(validate_account_settings(&account).is_ok());
    }

    #[test]
    fn imap_smtp_account_requires_sender_email_pattern() {
        let account = imap_smtp_account("alice-login", vec!["*@example.com"]);

        let error = validate_account_settings(&account).expect_err("validation should fail");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.body.code, "invalid_account");
        assert!(error.body.message.contains("sender email"));
    }

    #[test]
    fn imap_smtp_account_allows_username_with_sender_email_pattern() {
        let account = imap_smtp_account("alice-login", vec!["alice@example.com"]);

        assert!(validate_account_settings(&account).is_ok());
    }

    #[test]
    fn imap_smtp_account_rejects_email_username_without_sender_email_pattern() {
        let account = imap_smtp_account("alice@example.com", Vec::new());

        let error = validate_account_settings(&account).expect_err("validation should fail");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.body.code, "invalid_account");
        assert!(error.body.message.contains("sender email"));
    }

    #[test]
    fn secret_replace_requires_password() {
        let error = validate_secret_request(&SecretWriteRequest {
            mode: SecretWriteMode::Replace,
            password: None,
        })
        .expect_err("validation should fail");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.body.code, "invalid_secret");
    }

    #[test]
    fn secret_status_redacts_os_reference() {
        let status = secret_status(Some(&SecretRef {
            kind: SecretKind::Os,
            key: "account:primary".to_string(),
        }));

        assert_eq!(status.storage, SecretStorage::Os);
        assert!(status.configured);
        assert_eq!(status.label, None);
    }

    #[test]
    fn patch_account_preserves_username_when_username_is_omitted() {
        let mut account = AccountSettings {
            id: AccountId::from("primary"),
            name: "Primary".to_string(),
            full_name: None,
            email_patterns: Vec::new(),
            driver: AccountDriver::Jmap,
            enabled: true,
            appearance: None,
            transport: posthaste_domain::AccountTransportSettings {
                base_url: Some("https://before.example/jmap".to_string()),
                username: Some("alice@example.com".to_string()),
                secret_ref: None,
                ..Default::default()
            },
            created_at: "2026-03-31T10:00:00Z".to_string(),
            updated_at: "2026-03-31T10:00:00Z".to_string(),
        };

        apply_account_patch(
            &mut account,
            &PatchAccountRequest {
                name: None,
                full_name: None,
                email_patterns: None,
                driver: None,
                enabled: None,
                appearance: None,
                transport: Some(AccountTransportRequest {
                    base_url: Some("https://after.example/jmap".to_string()),
                    username: None,
                    ..Default::default()
                }),
                secret: None,
            },
        );

        assert_eq!(
            account.transport.base_url.as_deref(),
            Some("https://after.example/jmap")
        );
        assert_eq!(
            account.transport.username.as_deref(),
            Some("alice@example.com")
        );
    }

    #[test]
    fn account_appearance_accepts_camel_case_json() {
        let payload = r#"{"kind":"initials","initials":"P","colorHue":245}"#;
        let appearance: AccountAppearance =
            serde_json::from_str(payload).expect("camelCase appearance should deserialize");

        assert_eq!(
            appearance,
            AccountAppearance::Initials {
                initials: "P".to_string(),
                color_hue: 245,
            }
        );
    }

    #[test]
    fn provider_oauth_account_uses_identity_for_username_and_sender_address() {
        let account = match oauth_account_settings(
            AccountId::from("user-example-com"),
            ProviderHint::Gmail,
            "user@example.com".to_string(),
            "user@example.com".to_string(),
            vec!["user@example.com".to_string()],
            SecretRef {
                kind: SecretKind::Os,
                key: "account:user-example-com".to_string(),
            },
            "2026-04-27T10:00:00Z".to_string(),
        ) {
            Ok(account) => account,
            Err(error) => panic!(
                "OAuth account settings should build, got {}",
                error.into_response().status()
            ),
        };

        assert_eq!(account.driver, AccountDriver::ImapSmtp);
        assert_eq!(account.transport.auth, ProviderAuthKind::OAuth2);
        assert_eq!(
            account.transport.username.as_deref(),
            Some("user@example.com")
        );
        assert_eq!(account.email_patterns, vec!["user@example.com"]);
        assert!(validate_account_settings(&account).is_ok());
    }

    #[test]
    fn provider_oauth_account_sets_known_mail_endpoints() {
        let (gmail_imap, gmail_smtp) = match oauth_provider_mail_transport(&ProviderHint::Gmail) {
            Ok(transport) => transport,
            Err(error) => panic!(
                "Gmail transport should build, got {}",
                error.into_response().status()
            ),
        };
        let (outlook_imap, outlook_smtp) =
            match oauth_provider_mail_transport(&ProviderHint::Outlook) {
                Ok(transport) => transport,
                Err(error) => panic!(
                    "Outlook transport should build, got {}",
                    error.into_response().status()
                ),
            };

        assert_eq!(gmail_imap.host, "imap.gmail.com");
        assert_eq!(gmail_imap.security, TransportSecurity::Tls);
        assert_eq!(gmail_smtp.host, "smtp.gmail.com");
        assert_eq!(gmail_smtp.security, TransportSecurity::StartTls);
        assert_eq!(outlook_imap.host, "outlook.office365.com");
        assert_eq!(outlook_smtp.host, "smtp.office365.com");
    }

    fn imap_smtp_account(username: &str, email_patterns: Vec<&str>) -> AccountSettings {
        AccountSettings {
            id: AccountId::from("primary"),
            name: "Primary".to_string(),
            full_name: None,
            email_patterns: email_patterns.into_iter().map(str::to_string).collect(),
            driver: AccountDriver::ImapSmtp,
            enabled: true,
            appearance: None,
            transport: posthaste_domain::AccountTransportSettings {
                username: Some(username.to_string()),
                secret_ref: Some(SecretRef {
                    kind: SecretKind::Env,
                    key: "POSTHASTE_IMAP_PASSWORD".to_string(),
                }),
                imap: Some(ImapTransportSettings {
                    host: "imap.example.com".to_string(),
                    port: 993,
                    security: posthaste_domain::TransportSecurity::Tls,
                }),
                smtp: Some(SmtpTransportSettings {
                    host: "smtp.example.com".to_string(),
                    port: 587,
                    security: posthaste_domain::TransportSecurity::StartTls,
                }),
                ..Default::default()
            },
            created_at: "2026-03-31T10:00:00Z".to_string(),
            updated_at: "2026-03-31T10:00:00Z".to_string(),
        }
    }
}
