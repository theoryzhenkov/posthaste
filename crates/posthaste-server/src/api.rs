use std::collections::BTreeMap;
use std::convert::Infallible;
use std::io;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{Extension, Path, Query, State};
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
    ImapTransportSettings, MailboxId, MailboxRole, MailboxSummary, MessageAttachment,
    MessageCursor, MessageDetail, MessageId, MessagePage, MessageSortField, MessageSummary,
    ProviderAuthKind, ProviderHint, Recipient, RemoveFromMailboxCommand, ReplaceMailboxesCommand,
    ReplyContext, SecretKind, SecretRef, SecretStatus, SecretStorage, SendMessageRequest,
    ServiceError, ServiceErrorKind, SetKeywordsCommand, SharedGateway, SmartMailbox,
    SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup, SmartMailboxGroupOperator,
    SmartMailboxId, SmartMailboxKind, SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode,
    SmartMailboxSummary, SmartMailboxValue, SmtpTransportSettings, SortDirection, SyncMode,
    SyncTrigger, TagSummary, EVENT_TOPIC_ACCOUNT_CREATED, EVENT_TOPIC_ACCOUNT_DELETED,
    EVENT_TOPIC_ACCOUNT_UPDATED, EVENT_TOPIC_CONFIG_RELOADED, EVENT_TOPIC_SETTINGS_UPDATED,
    EVENT_TOPIC_SMART_MAILBOX_CREATED, EVENT_TOPIC_SMART_MAILBOX_DELETED,
    EVENT_TOPIC_SMART_MAILBOX_RESET, EVENT_TOPIC_SMART_MAILBOX_UPDATED,
};
use posthaste_observability::{events, ph_warn};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::fs;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use utoipa::{IntoParams, ToSchema};

const MAX_SEND_ATTACHMENTS: usize = 10;
const MAX_SEND_ATTACHMENT_BYTES: u64 = 10 * 1024 * 1024;
const MAX_SEND_TOTAL_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;

use crate::authz::Action;
use crate::oauth::{
    OAuthAuthorizationCodeExchange, OAuthExchangeResult, OAuthFlowCompletion, OAuthProviderProfile,
    OAuthTokenService, OAuthTokenSet, PendingOAuthFlow,
};
use crate::{observability, sanitize, AppState};

mod account_support;
pub(crate) mod accounts;
pub(crate) mod auth_tokens;
mod cursor_support;
pub mod message_commands;
pub(crate) mod read_calls;
pub mod settings;
pub mod smart_mailboxes;

pub use accounts::{
    complete_account_oauth, create_account, delete_account, disable_account, enable_account,
    get_account, get_account_logo, list_accounts, patch_account, reload_config,
    start_account_oauth, start_provider_oauth, upload_account_logo, verify_account,
    AccountTransportRequest, CreateAccountRequest, OAuthCallbackQuery, PatchAccountRequest,
    SecretWriteMode, SecretWriteRequest, StartOAuthRequest, StartOAuthResponse,
    StartProviderOAuthRequest,
};
pub use auth_tokens::{create_auth_token, CreateAuthTokenRequest, CreateAuthTokenResponse};
pub use message_commands::{
    add_to_mailbox, destroy_message, remove_from_mailbox, replace_mailboxes, set_keywords,
};
pub use read_calls::{
    read, AccountIdSelector, AccountListReadResult, MailboxListReadResult, ReadCall, ReadCallArgs,
    ReadOperation, ReadRequest, ReadResponse, ReadResult, SmartMailboxListReadResult,
    TagListReadResult,
};
pub use settings::{get_settings, patch_settings, preview_automation_rule};
pub use smart_mailboxes::{
    create_smart_mailbox, delete_smart_mailbox, get_smart_mailbox,
    list_smart_mailbox_conversations, list_smart_mailbox_messages, list_smart_mailboxes,
    patch_smart_mailbox, reset_default_smart_mailboxes,
};

use account_support::{
    account_overview, account_secret_ref, append_and_publish_account_event,
    append_and_publish_config_event, apply_account_patch, apply_secret_instruction,
    decide_secret_instruction, default_account_appearance, delete_managed_secret,
    generate_account_id_seed, generate_smart_mailbox_id, internal_error,
    normalize_account_appearance, normalize_automation_rules, normalize_email_patterns,
    normalize_optional, store_error_to_api, validate_account_settings, validate_automation_drafts,
    validate_automation_rules, validate_logo_image_id, ResourceChange, ResourceOperation,
};
use cursor_support::{
    conversation_limit, conversation_page_response, event_to_sse, matches_event, message_limit,
    message_page_response, parse_conversation_cursor, parse_message_cursor,
};

/// Product API readiness response.
///
/// @spec docs/L1-api#health
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: &'static str,
}

/// Request body for a manual source sync command.
///
/// @spec docs/L1-api#sync-and-events
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TriggerSyncRequest {
    #[serde(default)]
    pub mode: SyncMode,
}

/// Query parameters for conversation list endpoints.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Debug, Deserialize, IntoParams)]
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
#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListSourceMessagesQuery {
    pub mailbox_id: Option<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub sort: Option<MessageSortField>,
    pub sort_dir: Option<SortDirection>,
    pub q: Option<String>,
}

/// Query parameters for global message search.
///
/// @spec docs/L1-api#conversations-and-messages
#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct SearchMessagesQuery {
    pub q: String,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub sort: Option<MessageSortField>,
    pub sort_dir: Option<SortDirection>,
}

/// Query parameters for smart-mailbox message listing.
///
/// @spec docs/L1-api#smart-mailboxes
#[derive(Debug, Deserialize, IntoParams)]
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
#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct EventsQuery {
    pub account_id: Option<String>,
    pub topic: Option<String>,
    pub mailbox_id: Option<String>,
    pub after_seq: Option<i64>,
}

#[derive(Debug, Deserialize, IntoParams)]
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
        .map_err(|msg| ApiError::new(StatusCode::BAD_REQUEST, ApiErrorCode::InvalidQuery, msg))
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
    operation_id: Option<&str>,
) {
    let total_messages = match state.service.count_messages_by_rule(scope_rule) {
        Ok((_, total)) => total.max(0) as u64,
        Err(error) => {
            ph_warn!(
                events::CACHE_SEARCH_VISIBILITY_SCOPE_COUNT_FAILED,
                error = %error,
                "skipping cache search visibility signals because scope count failed"
            );
            return;
        }
    };
    let result_count = match state.service.count_messages_by_rule(result_rule) {
        Ok((_, total)) => total.max(0) as u64,
        Err(error) => {
            ph_warn!(
                events::CACHE_SEARCH_VISIBILITY_RESULT_COUNT_FAILED,
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
                ph_warn!(
                    events::CACHE_SEARCH_VISIBILITY_RECORD_FAILED,
                    error = %error,
                    "failed to record cache search visibility signals"
                );
                return;
            }
        };
    for account_id in account_ids {
        if let Err(error) = state
            .supervisor
            .trigger_cache_maintenance(&account_id, operation_id.map(str::to_string))
            .await
        {
            ph_warn!(
                events::CACHE_MAINTENANCE_TRIGGER_FAILED,
                account_id = %account_id,
                error = %error,
                "failed to trigger cache maintenance after search visibility signal"
            );
        }
    }
}

fn spawn_search_cache_visibility(
    state: Arc<AppState>,
    page: MessagePage,
    scope_rule: SmartMailboxRule,
    result_rule: SmartMailboxRule,
    operation_id: Option<String>,
) {
    tokio::spawn(async move {
        record_search_cache_visibility(
            &state,
            &page,
            &scope_rule,
            &result_rule,
            operation_id.as_deref(),
        )
        .await;
    });
}

/// Request body for `PATCH /v1/sources/{source_id}/mailboxes/{mailbox_id}`.
///
/// Outer `Option` distinguishes omitted `role` from an explicit JSON `null`.
///
/// @spec docs/L1-api#conversations-and-messages
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchMailboxRequest {
    pub role: Option<Option<String>>,
}

const MAX_ACCOUNT_LOGO_BYTES: usize = 2 * 1024 * 1024;

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

/// Request body for `POST /v1/smart-mailboxes`.
///
/// @spec docs/L1-api#smart-mailbox-crud
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateSmartMailboxRequest {
    pub name: String,
    pub position: Option<i64>,
    pub rule: SmartMailboxRule,
}

/// Request body for `PATCH /v1/smart-mailboxes/{id}`. Omitted fields are preserved.
///
/// @spec docs/L1-api#smart-mailbox-crud
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchSmartMailboxRequest {
    pub name: Option<String>,
    pub position: Option<i64>,
    pub rule: Option<SmartMailboxRule>,
}

/// Stable machine-readable API error code.
///
/// The single typed code space for the `/v1` surface: boundary-validation codes
/// raised by the API layer, plus the domain [`ServiceErrorKind`] codes mapped via
/// [`From<ServiceErrorKind>`]. Serializes to snake_case wire strings.
///
/// @spec docs/L1-api#error-format
/// @spec docs/L1-api#error-code-mapping
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    // Boundary validation.
    InvalidQuery,
    InvalidCursor,
    InvalidLimit,
    InvalidMailbox,
    InvalidCompose,
    InvalidSecret,
    InvalidProvider,
    InvalidAccount,
    InvalidAccountLogo,
    InvalidOauthRequest,
    InvalidOauthCallback,
    // OAuth outcomes.
    OauthDenied,
    InvalidGrant,
    // Account-field validation (the split of the old generic `invalid_account`).
    AccountBaseUrlRequired,
    AccountSecretRequired,
    AccountUsernameRequired,
    AccountSenderRequired,
    // Generic.
    NotFound,
    Conflict,
    InternalError,
    // Authentication / authorization (loopback trust model, default-off).
    Unauthorized,
    Forbidden,
    // Domain (mapped from `ServiceErrorKind`).
    GatewayUnavailable,
    AuthError,
    NetworkError,
    StateMismatch,
    CannotCalculateChanges,
    GatewayRejected,
    SecretUnavailable,
    SecretUnsupported,
    StorageFailure,
    ConfigValidation,
    ConfigIo,
    ConfigParse,
}

impl From<ServiceErrorKind> for ApiErrorCode {
    fn from(kind: ServiceErrorKind) -> Self {
        match kind {
            ServiceErrorKind::GatewayUnavailable => Self::GatewayUnavailable,
            ServiceErrorKind::AuthError => Self::AuthError,
            ServiceErrorKind::NetworkError => Self::NetworkError,
            ServiceErrorKind::StateMismatch => Self::StateMismatch,
            ServiceErrorKind::CannotCalculateChanges => Self::CannotCalculateChanges,
            ServiceErrorKind::GatewayRejected => Self::GatewayRejected,
            ServiceErrorKind::SecretUnavailable => Self::SecretUnavailable,
            ServiceErrorKind::SecretUnsupported => Self::SecretUnsupported,
            ServiceErrorKind::NotFound => Self::NotFound,
            ServiceErrorKind::Conflict => Self::Conflict,
            ServiceErrorKind::StorageFailure => Self::StorageFailure,
            ServiceErrorKind::ConfigValidation => Self::ConfigValidation,
            ServiceErrorKind::ConfigIo => Self::ConfigIo,
            ServiceErrorKind::ConfigParse => Self::ConfigParse,
        }
    }
}

/// JSON error response body returned by all API error paths.
///
/// @spec docs/L1-api#error-format
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorBody {
    /// Stable machine-readable error code.
    pub code: ApiErrorCode,
    /// Human-readable description of the failure.
    pub message: String,
    /// Optional structured context for the error.
    #[schema(value_type = Object)]
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
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OkResponse {
    pub ok: bool,
}

/// Response from `POST /v1/accounts/{id}/verify`.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResponse {
    pub ok: bool,
    pub identity_email: Option<String>,
    pub push_supported: bool,
}

/// Response from `POST /v1/sources/{id}/commands/sync`.
///
/// @spec docs/L1-api#sync-and-events
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TriggerSyncResponse {
    pub ok: bool,
    pub event_count: usize,
    pub mode: String,
}

/// Paginated conversation list response with an opaque cursor for the next page.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPageResponse {
    pub items: Vec<ConversationSummary>,
    pub next_cursor: Option<String>,
}

/// Paginated message list response with an opaque cursor for the next page.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MessagePageResponse {
    pub items: Vec<MessageSummary>,
    pub next_cursor: Option<String>,
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

impl ApiError {
    /// Map a domain `ServiceError` to an HTTP status code and JSON error body.
    ///
    /// @spec docs/L1-api#error-code-mapping
    pub fn from_service_error(error: ServiceError) -> Self {
        let status = service_error_status(error.kind());
        Self {
            status,
            body: ApiErrorBody {
                code: ApiErrorCode::from(error.kind()),
                message: error.to_string(),
                details: json!({}),
            },
        }
    }

    /// Construct an `ApiError` with explicit status, code, and message.
    pub fn new(status: StatusCode, code: ApiErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,
            body: ApiErrorBody {
                code,
                message: message.into(),
                details: json!({}),
            },
        }
    }
}

fn service_error_status(kind: ServiceErrorKind) -> StatusCode {
    match kind {
        ServiceErrorKind::NotFound => StatusCode::NOT_FOUND,
        ServiceErrorKind::Conflict | ServiceErrorKind::StateMismatch => StatusCode::CONFLICT,
        ServiceErrorKind::AuthError => StatusCode::UNAUTHORIZED,
        ServiceErrorKind::GatewayUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        ServiceErrorKind::NetworkError => StatusCode::BAD_GATEWAY,
        ServiceErrorKind::GatewayRejected
        | ServiceErrorKind::SecretUnavailable
        | ServiceErrorKind::SecretUnsupported
        | ServiceErrorKind::ConfigValidation
        | ServiceErrorKind::ConfigParse => StatusCode::BAD_REQUEST,
        ServiceErrorKind::CannotCalculateChanges
        | ServiceErrorKind::StorageFailure
        | ServiceErrorKind::ConfigIo => StatusCode::INTERNAL_SERVER_ERROR,
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

fn account_not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        ApiErrorCode::NotFound,
        "account not found",
    )
}

fn load_account(state: &AppState, account_id: &AccountId) -> Result<AccountSettings, ApiError> {
    state
        .service
        .get_source(account_id)
        .map_err(ApiError::from_service_error)?
        .ok_or_else(account_not_found)
}

fn command_result_response(
    state: &AppState,
    result: Result<CommandResult, ServiceError>,
) -> Result<Json<CommandResult>, ApiError> {
    let result = result.map_err(ApiError::from_service_error)?;
    state.publish_events(&result.events);
    Ok(Json(result))
}

/// GET /v1/health
///
/// @spec docs/L1-api#health
#[utoipa::path(
    get,
    path = "/v1/health",
    tag = "system",
    summary = "Service health check",
    description = "Returns readiness status. Used by clients to confirm the backend is reachable.",
    responses(
        (status = 200, description = "Service is ready", body = HealthResponse)
    )
)]
pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// GET /v1/sources/{source_id}/mailboxes
///
/// @spec docs/L1-api#conversations-and-messages
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/mailboxes",
    tag = "mailboxes",
    summary = "List mailboxes",
    description = "Returns all mailboxes for a source with unread and total counts.",
    params(("source_id" = String, Path, description = "Source (account) identifier")),
    responses(
        (status = 200, description = "Mailboxes for the source", body = [MailboxSummary]),
        (status = 404, description = "Source not found", body = ApiErrorBody)
    )
)]
pub async fn list_mailboxes(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
) -> Result<Json<Vec<MailboxSummary>>, ApiError> {
    let account_id = AccountId(source_id);
    load_account(state.as_ref(), &account_id)?;
    state
        .service
        .list_mailboxes(&account_id)
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// PATCH /v1/sources/{source_id}/mailboxes/{mailbox_id}
///
/// @spec docs/L1-api#conversations-and-messages
/// @spec docs/L1-jmap#methods-used
#[utoipa::path(
    patch,
    path = "/v1/sources/{source_id}/mailboxes/{mailbox_id}",
    tag = "mailboxes",
    summary = "Update mailbox role",
    description = "Sets or clears the role assigned to a mailbox and returns the updated list.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("mailbox_id" = String, Path, description = "Mailbox identifier")
    ),
    request_body = PatchMailboxRequest,
    responses(
        (status = 200, description = "Updated mailboxes for the source", body = [MailboxSummary]),
        (status = 400, description = "Invalid mailbox role", body = ApiErrorBody),
        (status = 404, description = "Source or mailbox not found", body = ApiErrorBody),
        (status = 503, description = "Account gateway unavailable", body = ApiErrorBody)
    )
)]
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
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/messages",
    tag = "messages",
    summary = "List source messages",
    description = "Returns a paginated page of message summaries for a source, optionally filtered \
                   by mailbox or a search query.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ListSourceMessagesQuery
    ),
    responses(
        (status = 200, description = "A page of message summaries", body = MessagePageResponse),
        (status = 400, description = "Invalid cursor or query", body = ApiErrorBody),
        (status = 404, description = "Source not found", body = ApiErrorBody)
    )
)]
pub async fn list_source_messages(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<ListSourceMessagesQuery>,
) -> Result<Json<MessagePageResponse>, ApiError> {
    let mailbox_id = query.mailbox_id.map(MailboxId);
    let limit = message_limit(query.limit)?;
    let cursor = parse_message_cursor(query.cursor.as_deref())?;
    let account_id = AccountId::from(source_id.as_str());
    load_account(state.as_ref(), &account_id)?;
    validate_source_message_cursor(&account_id, cursor.as_ref())?;
    let sort_field = query.sort.unwrap_or_default();
    let sort_direction = query.sort_dir.unwrap_or_default();
    if let Some(search_rule) = parse_optional_search_rule(query.q.as_deref())? {
        let scoped_rule = source_message_scope_rule(account_id.as_str(), mailbox_id.as_ref());
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
        let operation_id = observability::operation_id_from_headers(&headers);
        spawn_search_cache_visibility(
            Arc::clone(&state),
            page.clone(),
            scoped_rule,
            result_rule,
            operation_id,
        );
        return Ok(Json(message_page_response(page)));
    }
    let page = state
        .service
        .list_message_page(
            &account_id,
            mailbox_id.as_ref(),
            limit,
            cursor.as_ref(),
            sort_field,
            sort_direction,
        )
        .map_err(ApiError::from_service_error)?;
    Ok(Json(message_page_response(page)))
}

fn validate_source_message_cursor(
    account_id: &AccountId,
    cursor: Option<&MessageCursor>,
) -> Result<(), ApiError> {
    if cursor.is_some_and(|cursor| &cursor.source_id != account_id) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidCursor,
            "cursor does not belong to requested source",
        ));
    }
    Ok(())
}

/// GET /v1/messages/search
///
/// Returns a global, paginated message search page without source fan-out.
///
/// @spec docs/L1-api#conversations-and-messages
/// @spec docs/L1-api#cursor-pagination
#[utoipa::path(
    get,
    path = "/v1/messages/search",
    tag = "messages",
    summary = "Search messages",
    description = "Returns a global, paginated message search page without source fan-out.",
    params(SearchMessagesQuery),
    responses(
        (status = 200, description = "A page of matching message summaries", body = MessagePageResponse),
        (status = 400, description = "Invalid or empty query", body = ApiErrorBody)
    )
)]
pub async fn search_messages(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchMessagesQuery>,
) -> Result<Json<MessagePageResponse>, ApiError> {
    let limit = message_limit(query.limit)?;
    let cursor = parse_message_cursor(query.cursor.as_deref())?;
    let sort_field = query.sort.unwrap_or_default();
    let sort_direction = query.sort_dir.unwrap_or_default();
    let rule = parse_optional_search_rule(Some(query.q.as_str()))?.ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidQuery,
            "search query must not be empty",
        )
    })?;
    state
        .service
        .query_message_page_by_rule(&rule, limit, cursor.as_ref(), sort_field, sort_direction)
        .map(message_page_response)
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// GET /v1/views/conversations
///
/// @spec docs/L1-api#conversations-and-messages
/// @spec docs/L1-api#cursor-pagination
#[utoipa::path(
    get,
    path = "/v1/views/conversations",
    tag = "conversations",
    summary = "List conversations",
    description = "Returns a paginated page of conversation summaries, optionally filtered by \
                   source, mailbox, or a search query.",
    params(ListConversationsQuery),
    responses(
        (status = 200, description = "A page of conversation summaries", body = ConversationPageResponse),
        (status = 400, description = "Invalid cursor or query", body = ApiErrorBody)
    )
)]
pub async fn list_conversations(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListConversationsQuery>,
) -> Result<Json<ConversationPageResponse>, ApiError> {
    let limit = conversation_limit(query.limit)?;
    let cursor = parse_conversation_cursor(query.cursor.as_deref())?;
    let sort_field = query.sort.unwrap_or_default();
    let sort_direction = query.sort_dir.unwrap_or_default();

    // When a search query is provided, parse it into a rule. If the request also
    // carries a `sourceId` (optionally `mailboxId`) filter, AND a scope rule into
    // it so the search is restricted to that account — exactly as the non-search
    // branch restricts via `list_conversations`.
    //
    // SECURITY: this scope rule is what makes the route safe to map as a Filter on
    // `sourceId`. An account-scoped capability token is required by the auth layer
    // to carry a matching `?sourceId`; without this, the search branch would
    // return cross-account results and the token's `account` caveat would be
    // meaningless. Do not drop it.
    if let Some(q) = &query.q {
        if !q.trim().is_empty() {
            let search_rule = parse_optional_search_rule(Some(q))?.expect("non-empty query");
            let rule = match query.source_id.as_deref() {
                Some(source_id) => {
                    let mailbox_id = query.mailbox_id.as_deref().map(MailboxId::from);
                    combine_rules(vec![
                        source_message_scope_rule(source_id, mailbox_id.as_ref()),
                        search_rule,
                    ])
                }
                None => search_rule,
            };
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
#[utoipa::path(
    get,
    path = "/v1/views/conversations/{conversation_id}",
    tag = "conversations",
    summary = "Get conversation",
    description = "Returns a full conversation with all messages expanded.",
    params(("conversation_id" = String, Path, description = "Conversation identifier")),
    responses(
        (status = 200, description = "The conversation detail", body = ConversationView),
        (status = 404, description = "Conversation not found", body = ApiErrorBody)
    )
)]
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
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/messages/{message_id}",
    tag = "messages",
    summary = "Get message detail",
    description = "Returns full message detail with sanitized body HTML and rewritten inline \
                   attachment URLs.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("message_id" = String, Path, description = "Message identifier")
    ),
    responses(
        (status = 200, description = "The message detail", body = MessageDetail),
        (status = 404, description = "Message not found", body = ApiErrorBody)
    )
)]
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
            ApiErrorCode::NotFound,
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
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/messages/{message_id}/attachments/{attachment_id}",
    tag = "messages",
    summary = "Get message attachment",
    description = "Returns the raw bytes of a message attachment, inline or as a download.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("message_id" = String, Path, description = "Message identifier"),
        ("attachment_id" = String, Path, description = "Attachment identifier"),
        GetAttachmentQuery
    ),
    responses(
        (status = 200, description = "Attachment bytes, served with the attachment's own MIME type (octet-stream fallback)", content_type = "*/*", body = [u8]),
        (status = 404, description = "Message or attachment not found", body = ApiErrorBody),
        (status = 502, description = "Upstream network error fetching the attachment", body = ApiErrorBody),
        (status = 503, description = "Account gateway unavailable", body = ApiErrorBody)
    )
)]
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
            ApiErrorCode::NotFound,
            "message detail not available",
        )
    })?;
    let attachment = detail
        .attachments
        .into_iter()
        .find(|attachment| attachment.id == attachment_id)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                ApiErrorCode::NotFound,
                "attachment not found",
            )
        })?;
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
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/identity",
    tag = "messages",
    summary = "Get sender identity",
    description = "Returns the JMAP sender identity for a source.",
    params(("source_id" = String, Path, description = "Source (account) identifier")),
    responses(
        (status = 200, description = "The sender identity", body = Identity),
        (status = 404, description = "Source not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
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
#[utoipa::path(
    get,
    path = "/v1/sender-addresses",
    tag = "messages",
    summary = "List sender addresses",
    description = "Returns locally cached sender addresses that previously passed submission.",
    responses(
        (status = 200, description = "Cached sender addresses", body = [CachedSenderAddress]),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
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
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/messages/{message_id}/reply-context",
    tag = "messages",
    summary = "Get reply context",
    description = "Returns pre-computed reply/forward metadata (recipients, subjects, quoted body).",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("message_id" = String, Path, description = "Message identifier")
    ),
    responses(
        (status = 200, description = "The reply context", body = ReplyContext),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
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
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/commands/send",
    tag = "messages",
    summary = "Send message",
    description = "Validates and submits a new email via the source gateway, then triggers a sync.",
    params(("source_id" = String, Path, description = "Source (account) identifier")),
    request_body = SendMessageRequest,
    responses(
        (status = 200, description = "Message accepted for delivery", body = OkResponse),
        (status = 400, description = "Invalid compose request", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
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
            ph_warn!(
                events::SEND_SENDER_CACHE_UPDATE_FAILED,
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
        ph_warn!(
            events::SEND_FOLLOWUP_SYNC_TRIGGER_FAILED,
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
            ApiErrorCode::InvalidMailbox,
            "role is required",
        ));
    };
    match role.as_deref() {
        None => Ok(role),
        Some(value) if MailboxRole::parse(value).is_some() => Ok(role),
        Some(_) => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidMailbox,
            "unsupported mailbox role",
        )),
    }
}

fn validate_send_message_request(request: &SendMessageRequest) -> Result<(), ApiError> {
    if request.from.as_ref().is_some_and(recipient_email_is_empty) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidCompose,
            "sender email address cannot be empty",
        ));
    }
    if request.to.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidCompose,
            "at least one To recipient is required",
        ));
    }
    if request.subject.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidCompose,
            "subject is required",
        ));
    }
    if request.body.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidCompose,
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
            ApiErrorCode::InvalidCompose,
            "recipient email addresses cannot be empty",
        ));
    }
    if request.attachments.len() > MAX_SEND_ATTACHMENTS {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidCompose,
            "too many attachments",
        ));
    }
    let mut total_attachment_bytes = 0_u64;
    for attachment in &request.attachments {
        if attachment.filename.trim().is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidCompose,
                "attachment filename is required",
            ));
        }
        let attachment_size = decoded_attachment_size(attachment.content_base64.trim())
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    ApiErrorCode::InvalidCompose,
                    "attachment content must be valid base64",
                )
            })?;
        if attachment_size > MAX_SEND_ATTACHMENT_BYTES {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidCompose,
                "attachment is too large",
            ));
        }
        total_attachment_bytes = total_attachment_bytes.saturating_add(attachment_size);
        if total_attachment_bytes > MAX_SEND_TOTAL_ATTACHMENT_BYTES {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidCompose,
                "attachments are too large",
            ));
        }
    }
    Ok(())
}

fn recipient_email_is_empty(recipient: &Recipient) -> bool {
    recipient.email.trim().is_empty()
}

fn decoded_attachment_size(content: &str) -> Option<u64> {
    let mut decoder = base64::read::DecoderReader::new(
        content.as_bytes(),
        &base64::engine::general_purpose::STANDARD,
    );
    io::copy(&mut decoder, &mut io::sink()).ok()
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

/// POST /v1/sources/{source_id}/commands/sync
///
/// @spec docs/L1-api#sync-and-events
/// @spec docs/L1-sync#sync-loop
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/commands/sync",
    tag = "sync",
    summary = "Trigger sync",
    description = "Runs a manual sync for a source and reports the number of events emitted.",
    params(("source_id" = String, Path, description = "Source (account) identifier")),
    // NOTE: the handler accepts an absent body (defaults to incremental sync). utoipa's
    // path macro can't emit `requestBody.required: false`, so optionality is documented here.
    request_body(content = TriggerSyncRequest,
        description = "Optional. Defaults to an incremental sync when the body is omitted."),
    responses(
        (status = 200, description = "Sync result", body = TriggerSyncResponse),
        (status = 404, description = "Source not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn trigger_sync(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    request: Option<Json<TriggerSyncRequest>>,
) -> Result<Json<TriggerSyncResponse>, ApiError> {
    let account_id = AccountId(source_id);
    let mode = request
        .map(|Json(request)| request.mode)
        .unwrap_or_default();
    let event_count = state
        .supervisor
        .sync_account_with_mode(&account_id, mode)
        .await
        .map_err(ApiError::from_service_error)?;
    Ok(Json(TriggerSyncResponse {
        ok: true,
        event_count,
        mode: mode.as_str().to_string(),
    }))
}

/// GET /v1/events
///
/// Opens an SSE stream. When `afterSeq` is provided, replays matching events
/// from the backlog before switching to the live broadcast stream.
///
/// @spec docs/L1-api#sse-event-stream
/// @spec docs/L0-api#server-sent-events-for-push
// NOTE: utoipa cannot infer the SSE payload type. The full event payload contract
// (DomainEvent over text/event-stream) is documented in P3 via AsyncAPI.
#[utoipa::path(
    get,
    path = "/v1/events",
    tag = "events",
    summary = "Stream events",
    description = "Opens a Server-Sent Events stream of domain events. When afterSeq is provided, \
                   replays matching backlog events before switching to the live stream.",
    params(EventsQuery),
    responses(
        (status = 200, description = "Server-sent event stream of domain events", content_type = "text/event-stream"),
        (status = 400, description = "Invalid filter", body = ApiErrorBody)
    )
)]
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

#[cfg(test)]
use accounts::{oauth_account_settings, oauth_provider_mail_transport};
#[cfg(test)]
use auth_tokens::{build_token_caveats, derive_capability_token};

#[cfg(test)]
mod tests;
