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
mod cursor_support;
pub mod message_commands;
pub mod settings;
pub mod smart_mailboxes;

pub use message_commands::{
    add_to_mailbox, destroy_message, remove_from_mailbox, replace_mailboxes, set_keywords,
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

/// Request body for the typed read-call endpoint.
///
/// @spec docs/L1-api#read-calls
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadRequest {
    pub calls: Vec<ReadCall>,
}

/// A single domain read operation requested as part of `POST /v1/read`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadCall {
    pub id: String,
    pub op: ReadOperation,
    #[serde(default)]
    pub args: ReadCallArgs,
}

/// Supported read operation names.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, ToSchema)]
pub enum ReadOperation {
    #[serde(rename = "Account/list")]
    AccountList,
    #[serde(rename = "Mailbox/list")]
    MailboxList,
    #[serde(rename = "SmartMailbox/list")]
    SmartMailboxList,
    #[serde(rename = "Tag/list")]
    TagList,
}

/// Optional read-call arguments. Only `Mailbox/list` currently uses `accountIds`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadCallArgs {
    pub account_ids: Option<AccountIdSelector>,
}

/// Account id selector for read calls. A string beginning with `#` is a result
/// reference such as `#accounts.ids`; an array is an explicit account-id list.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum AccountIdSelector {
    Explicit(Vec<String>),
    Reference(String),
}

/// Response body for the typed read-call endpoint.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReadResponse {
    pub results: BTreeMap<String, ReadResult>,
}

/// A successful read-call result, discriminated by operation name.
#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "op", content = "value")]
pub enum ReadResult {
    #[serde(rename = "Account/list")]
    AccountList(AccountListReadResult),
    #[serde(rename = "Mailbox/list")]
    MailboxList(MailboxListReadResult),
    #[serde(rename = "SmartMailbox/list")]
    SmartMailboxList(SmartMailboxListReadResult),
    #[serde(rename = "Tag/list")]
    TagList(TagListReadResult),
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountListReadResult {
    pub ids: Vec<AccountId>,
    pub enabled_ids: Vec<AccountId>,
    pub items: Vec<AccountOverview>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MailboxListReadResult {
    pub by_account_id: BTreeMap<AccountId, Vec<MailboxSummary>>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SmartMailboxListReadResult {
    pub items: Vec<SmartMailboxSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TagListReadResult {
    pub items: Vec<TagSummary>,
}

const MAX_READ_CALLS: usize = 16;

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

/// Transport fields for account create/patch requests.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Default, Deserialize, ToSchema)]
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
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, ToSchema)]
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
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretWriteRequest {
    #[serde(default)]
    pub mode: SecretWriteMode,
    pub password: Option<String>,
}

/// Request body for `POST /v1/accounts/{account_id}/oauth/start`.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartOAuthRequest {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
}

/// Request body for `POST /v1/oauth/start`.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize, ToSchema)]
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
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartOAuthResponse {
    pub authorization_url: String,
    pub state: String,
    pub redirect_uri: String,
}

/// Query parameters for the loopback OAuth redirect endpoint.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize, IntoParams)]
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
#[derive(Debug, Deserialize, ToSchema)]
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
#[derive(Debug, Deserialize, ToSchema)]
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

/// Find a free account id from `seed`, appending `-2`, `-3`, … on collision.
fn allocate_unique_account_id(state: &AppState, seed: &str) -> Result<AccountId, ApiError> {
    let mut candidate = AccountId::from(seed);
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
    Ok(candidate)
}

/// Persist a freshly-built account: save → start runtime → publish event.
///
/// If `save_source` fails after a secret was written to the keyring, roll the
/// secret back so a failed create does not orphan it (consistent across the
/// manual and OAuth creation paths). `delete_managed_secret` no-ops unless the
/// account carries an OS-managed secret.
async fn persist_new_account(
    state: &Arc<AppState>,
    account: &AccountSettings,
    topic: &str,
) -> Result<(), ApiError> {
    if let Err(error) = state.service.save_source(account) {
        delete_managed_secret(state, account.transport.secret_ref.as_ref())?;
        return Err(ApiError::from_service_error(error));
    }
    state.supervisor.start_account(account).await;
    append_and_publish_account_event(state, &account.id, topic).map_err(store_error_to_api)?;
    Ok(())
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

/// POST /v1/read
///
/// Executes typed, read-only domain operations in order. Later calls can refer
/// to earlier account-list results with references such as `#accounts.enabledIds`.
///
/// @spec docs/L1-api#read-calls
#[utoipa::path(
    post,
    path = "/v1/read",
    tag = "read",
    summary = "Execute typed read calls",
    description = "Executes a JMAP-style batch of typed, read-only domain operations.",
    request_body = ReadRequest,
    responses(
        (status = 200, description = "Read-call results keyed by call id", body = ReadResponse),
        (status = 400, description = "Invalid read call or result reference", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn read(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ReadRequest>,
) -> Result<Json<ReadResponse>, ApiError> {
    if request.calls.len() > MAX_READ_CALLS {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidQuery,
            format!("read request exceeds {MAX_READ_CALLS} calls"),
        ));
    }
    let mut results = BTreeMap::new();
    for call in request.calls {
        if call.id.trim().is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidQuery,
                "read call id must not be empty",
            ));
        }
        if results.contains_key(&call.id) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidQuery,
                "read call ids must be unique",
            ));
        }
        let id = call.id;
        let result = execute_read_call(&state, &results, call.op, call.args).await?;
        results.insert(id, result);
    }
    Ok(Json(ReadResponse { results }))
}

async fn execute_read_call(
    state: &Arc<AppState>,
    prior_results: &BTreeMap<String, ReadResult>,
    op: ReadOperation,
    args: ReadCallArgs,
) -> Result<ReadResult, ApiError> {
    match op {
        ReadOperation::AccountList => read_accounts(state).await.map(ReadResult::AccountList),
        ReadOperation::MailboxList => read_mailboxes(state, prior_results, args)
            .await
            .map(ReadResult::MailboxList),
        ReadOperation::SmartMailboxList => state
            .service
            .list_smart_mailboxes()
            .map(|items| ReadResult::SmartMailboxList(SmartMailboxListReadResult { items }))
            .map_err(ApiError::from_service_error),
        ReadOperation::TagList => read_tags(state, prior_results, args)
            .await
            .map(ReadResult::TagList),
    }
}

async fn read_accounts(state: &Arc<AppState>) -> Result<AccountListReadResult, ApiError> {
    let settings = state
        .service
        .get_app_settings()
        .map_err(ApiError::from_service_error)?;
    let accounts = state
        .service
        .list_sources()
        .map_err(ApiError::from_service_error)?;
    let mut ids = Vec::with_capacity(accounts.len());
    let mut enabled_ids = Vec::new();
    let mut items = Vec::with_capacity(accounts.len());
    for account in accounts {
        ids.push(account.id.clone());
        if account.enabled {
            enabled_ids.push(account.id.clone());
        }
        items.push(account_overview(state, &settings, account).await);
    }
    Ok(AccountListReadResult {
        ids,
        enabled_ids,
        items,
    })
}

async fn read_mailboxes(
    state: &Arc<AppState>,
    prior_results: &BTreeMap<String, ReadResult>,
    args: ReadCallArgs,
) -> Result<MailboxListReadResult, ApiError> {
    let account_ids = resolve_read_account_ids(state, prior_results, args.account_ids).await?;
    let mut by_account_id = BTreeMap::new();
    for account_id in account_ids {
        load_account(state.as_ref(), &account_id)?;
        let mailboxes = state
            .service
            .list_mailboxes(&account_id)
            .map_err(ApiError::from_service_error)?;
        by_account_id.insert(account_id, mailboxes);
    }
    Ok(MailboxListReadResult { by_account_id })
}

async fn read_tags(
    state: &Arc<AppState>,
    prior_results: &BTreeMap<String, ReadResult>,
    args: ReadCallArgs,
) -> Result<TagListReadResult, ApiError> {
    let account_ids = resolve_read_account_ids(state, prior_results, args.account_ids).await?;
    state
        .service
        .list_merged_tags(&account_ids)
        .map(|items| TagListReadResult { items })
        .map_err(ApiError::from_service_error)
}

async fn resolve_read_account_ids(
    state: &Arc<AppState>,
    prior_results: &BTreeMap<String, ReadResult>,
    selector: Option<AccountIdSelector>,
) -> Result<Vec<AccountId>, ApiError> {
    match selector {
        Some(AccountIdSelector::Explicit(ids)) => Ok(ids.into_iter().map(AccountId).collect()),
        Some(AccountIdSelector::Reference(reference)) => {
            resolve_account_id_reference(prior_results, &reference)
        }
        None => {
            let accounts = state
                .service
                .list_sources()
                .map_err(ApiError::from_service_error)?;
            Ok(accounts
                .into_iter()
                .filter(|account| account.enabled)
                .map(|account| account.id)
                .collect())
        }
    }
}

fn resolve_account_id_reference(
    prior_results: &BTreeMap<String, ReadResult>,
    reference: &str,
) -> Result<Vec<AccountId>, ApiError> {
    let Some(reference) = reference.strip_prefix('#') else {
        return Err(invalid_read_reference(reference));
    };
    let Some((call_id, field)) = reference.split_once('.') else {
        return Err(invalid_read_reference(reference));
    };
    let Some(ReadResult::AccountList(accounts)) = prior_results.get(call_id) else {
        return Err(invalid_read_reference(reference));
    };
    match field {
        "ids" => Ok(accounts.ids.clone()),
        "enabledIds" => Ok(accounts.enabled_ids.clone()),
        _ => Err(invalid_read_reference(reference)),
    }
}

fn invalid_read_reference(reference: &str) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        ApiErrorCode::InvalidQuery,
        format!("invalid read result reference: {reference}"),
    )
}

/// GET /v1/accounts
///
/// @spec docs/L1-api#accounts
#[utoipa::path(
    get,
    path = "/v1/accounts",
    tag = "accounts",
    summary = "List accounts",
    description = "Returns all configured accounts with their runtime overview.",
    responses(
        (status = 200, description = "All configured accounts", body = [AccountOverview]),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
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
#[utoipa::path(
    get,
    path = "/v1/accounts/{account_id}",
    tag = "accounts",
    summary = "Get account",
    description = "Returns a single account with its runtime overview.",
    params(("account_id" = String, Path, description = "Account identifier")),
    responses(
        (status = 200, description = "The requested account", body = AccountOverview),
        (status = 404, description = "Account not found", body = ApiErrorBody)
    )
)]
pub async fn get_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<AccountOverview>, ApiError> {
    let settings = state
        .service
        .get_app_settings()
        .map_err(ApiError::from_service_error)?;
    let account_id = AccountId::from(account_id.as_str());
    let account = load_account(state.as_ref(), &account_id)?;
    Ok(Json(account_overview(&state, &settings, account).await))
}

/// POST /v1/accounts
///
/// Validates uniqueness, applies secret instruction, persists config, starts
/// the supervisor runtime, and emits an `account.created` event.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[utoipa::path(
    post,
    path = "/v1/accounts",
    tag = "accounts",
    summary = "Create account",
    description = "Validates uniqueness, applies the secret instruction, persists config, \
                   starts the runtime, and emits an account.created event.",
    request_body = CreateAccountRequest,
    responses(
        (status = 200, description = "The created account", body = AccountOverview),
        (status = 400, description = "Validation failed", body = ApiErrorBody),
        (status = 409, description = "Account already exists", body = ApiErrorBody)
    )
)]
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
            allocate_unique_account_id(state.as_ref(), &seed)?
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
            ApiErrorCode::Conflict,
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
    account.transport.secret_ref =
        decide_secret_instruction(&account.id, None, &secret)?.resolved_secret_ref(None);
    validate_account_settings(&account)?;
    apply_secret_instruction(state.as_ref(), &mut account, None, &secret)?;
    persist_new_account(&state, &account, EVENT_TOPIC_ACCOUNT_CREATED).await?;

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
#[utoipa::path(
    patch,
    path = "/v1/accounts/{account_id}",
    tag = "accounts",
    summary = "Update account",
    description = "Sparse-merges provided fields into the existing account and restarts the runtime.",
    params(("account_id" = String, Path, description = "Account identifier")),
    request_body = PatchAccountRequest,
    responses(
        (status = 200, description = "The updated account", body = AccountOverview),
        (status = 400, description = "Validation failed", body = ApiErrorBody),
        (status = 404, description = "Account not found", body = ApiErrorBody)
    )
)]
pub async fn patch_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
    Json(request): Json<PatchAccountRequest>,
) -> Result<Json<AccountOverview>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let mut account = load_account(state.as_ref(), &account_id)?;
    let previous_image_id = account_appearance_image_id(&account);
    apply_account_patch(&mut account, &request);
    account.updated_at = domain_now_iso8601().map_err(internal_error)?;
    let existing_secret_ref = account.transport.secret_ref.clone();
    let secret_request = request.secret.unwrap_or_default();
    account.transport.secret_ref =
        decide_secret_instruction(&account.id, existing_secret_ref.as_ref(), &secret_request)?
            .resolved_secret_ref(existing_secret_ref.as_ref());
    validate_account_settings(&account)?;
    let defer_secret_clear = secret_request.mode == SecretWriteMode::Clear;
    if !defer_secret_clear {
        apply_secret_instruction(
            state.as_ref(),
            &mut account,
            existing_secret_ref.as_ref(),
            &secret_request,
        )?;
    }

    state
        .service
        .save_source(&account)
        .map_err(ApiError::from_service_error)?;
    if defer_secret_clear {
        apply_secret_instruction(
            state.as_ref(),
            &mut account,
            existing_secret_ref.as_ref(),
            &secret_request,
        )?;
    }
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
#[utoipa::path(
    post,
    path = "/v1/accounts/{account_id}/verify",
    tag = "accounts",
    summary = "Verify account",
    description = "Attempts JMAP session discovery and reports identity and push support.",
    params(("account_id" = String, Path, description = "Account identifier")),
    responses(
        (status = 200, description = "Verification result", body = VerificationResponse),
        (status = 404, description = "Account not found", body = ApiErrorBody),
        (status = 502, description = "Gateway verification failed", body = ApiErrorBody)
    )
)]
pub async fn verify_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<VerificationResponse>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let account = load_account(state.as_ref(), &account_id)?;
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
#[utoipa::path(
    post,
    path = "/v1/oauth/start",
    tag = "oauth",
    summary = "Start provider OAuth flow",
    description = "Creates a backend-held PKCE authorization session for provider-first setup.",
    request_body = StartProviderOAuthRequest,
    responses(
        (status = 200, description = "Authorization session details", body = StartOAuthResponse),
        (status = 400, description = "Invalid provider or request", body = ApiErrorBody)
    )
)]
pub async fn start_provider_oauth(
    State(state): State<Arc<AppState>>,
    Json(request): Json<StartProviderOAuthRequest>,
) -> Result<Json<StartOAuthResponse>, ApiError> {
    let profile = OAuthProviderProfile::for_provider(&request.provider).ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidProvider,
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
#[utoipa::path(
    post,
    path = "/v1/accounts/{account_id}/oauth/start",
    tag = "oauth",
    summary = "Start account OAuth flow",
    description = "Creates a backend-held PKCE authorization session for an existing account.",
    params(("account_id" = String, Path, description = "Account identifier")),
    request_body = StartOAuthRequest,
    responses(
        (status = 200, description = "Authorization session details", body = StartOAuthResponse),
        (status = 400, description = "Invalid account or request", body = ApiErrorBody),
        (status = 404, description = "Account not found", body = ApiErrorBody)
    )
)]
pub async fn start_account_oauth(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
    Json(request): Json<StartOAuthRequest>,
) -> Result<Json<StartOAuthResponse>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let account = load_account(state.as_ref(), &account_id)?;
    let profile =
        OAuthProviderProfile::for_provider(&account.transport.provider).ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidAccount,
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
#[utoipa::path(
    get,
    path = "/v1/oauth/callback",
    tag = "oauth",
    summary = "Complete OAuth flow",
    description = "Loopback redirect target. Exchanges a provider authorization code for a token \
                   set and returns an HTML page for the browser tab.",
    params(OAuthCallbackQuery),
    responses(
        (status = 200, description = "OAuth completion HTML page", content_type = "text/html"),
        (status = 400, description = "OAuth denied or invalid callback", body = ApiErrorBody)
    )
)]
pub async fn complete_account_oauth(
    State(state): State<Arc<AppState>>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Html<String>, ApiError> {
    if let Some(error) = query.error {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::OauthDenied,
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
                ApiErrorCode::InvalidOauthCallback,
                "OAuth callback is missing code",
            )
        })?;
    let flow = match state.oauth_flows.begin_completion(&query.state).await {
        OAuthFlowCompletion::Pending(flow) => flow,
        OAuthFlowCompletion::Completing => return Ok(oauth_processing_html()),
        OAuthFlowCompletion::Completed => return Ok(oauth_success_html()),
        OAuthFlowCompletion::Unknown => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidOauthCallback,
                "OAuth callback state is unknown or already used",
            ));
        }
    };
    let oauth = OAuthTokenService::new().map_err(ServiceError::from)?;
    let exchange = oauth
        .exchange_authorization_code(OAuthAuthorizationCodeExchange {
            profile: &flow.profile,
            client_id: &flow.client_id,
            client_secret: flow.client_secret.as_deref(),
            redirect_uri: &flow.redirect_uri,
            code,
            pkce_verifier: &flow.pkce_verifier,
            nonce: &flow.nonce,
            now: time::OffsetDateTime::now_utc(),
        })
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

    state.oauth_flows.mark_completed(query.state).await;
    Ok(oauth_success_html())
}

fn oauth_success_html() -> Html<String> {
    Html(
        "<!doctype html><meta charset=\"utf-8\"><title>Posthaste OAuth</title><p>Authentication complete. You can return to Posthaste.</p>".to_string(),
    )
}

fn oauth_processing_html() -> Html<String> {
    Html(
        "<!doctype html><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"1\"><title>Posthaste OAuth</title><p>Authentication is still completing. This page will refresh automatically.</p>".to_string(),
    )
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
            ApiErrorCode::InvalidOauthRequest,
            "clientId is required",
        ));
    }
    let redirect_uri = redirect_uri.trim();
    if redirect_uri.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidOauthRequest,
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
    let account_id = allocate_unique_account_id(state.as_ref(), &seed)?;

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
    persist_new_account(state, &account, EVENT_TOPIC_ACCOUNT_CREATED).await?;
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
    OAuthProviderProfile::for_provider(provider)
        .and_then(|profile| profile.default_mail_transport())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidProvider,
                "provider does not support built-in OAuth account creation",
            )
        })
}

async fn persist_oauth_token_set(
    state: &Arc<AppState>,
    account_id: &AccountId,
    token_set: OAuthTokenSet,
) -> Result<(), ApiError> {
    let mut account = load_account(state.as_ref(), account_id)?;
    let previous_secret_ref = account.transport.secret_ref.clone();
    let secret_ref = previous_secret_ref
        .as_ref()
        .filter(|secret_ref| matches!(secret_ref.kind, SecretKind::Os))
        .cloned()
        .unwrap_or_else(|| account_secret_ref(&account.id));
    let encoded = token_set.encode().map_err(ServiceError::from)?;

    account.transport.auth = ProviderAuthKind::OAuth2;
    account.transport.secret_ref = Some(secret_ref.clone());
    account.updated_at = domain_now_iso8601().map_err(internal_error)?;
    validate_account_settings(&account)?;

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
#[utoipa::path(
    post,
    path = "/v1/accounts/{account_id}/enable",
    tag = "accounts",
    summary = "Enable account",
    description = "Sets the account enabled flag and restarts the runtime.",
    params(("account_id" = String, Path, description = "Account identifier")),
    responses(
        (status = 200, description = "Account enabled", body = OkResponse),
        (status = 404, description = "Account not found", body = ApiErrorBody)
    )
)]
pub async fn enable_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    set_account_enabled(state, account_id, true).await
}

/// POST /v1/accounts/{account_id}/disable
///
/// @spec docs/L1-api#account-crud-lifecycle
#[utoipa::path(
    post,
    path = "/v1/accounts/{account_id}/disable",
    tag = "accounts",
    summary = "Disable account",
    description = "Clears the account enabled flag and restarts the runtime.",
    params(("account_id" = String, Path, description = "Account identifier")),
    responses(
        (status = 200, description = "Account disabled", body = OkResponse),
        (status = 404, description = "Account not found", body = ApiErrorBody)
    )
)]
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
#[utoipa::path(
    post,
    path = "/v1/accounts/{account_id}/logo",
    tag = "accounts",
    summary = "Upload account logo",
    description = "Stores a user-uploaded account logo (PNG, JPEG, WebP, or GIF) and updates the \
                   account appearance to reference it. The request body is the raw image bytes.",
    params(("account_id" = String, Path, description = "Account identifier")),
    request_body(content = [u8], description = "Raw image bytes", content_type = "application/octet-stream"),
    responses(
        (status = 200, description = "The updated account", body = AccountOverview),
        (status = 400, description = "Invalid or oversized image", body = ApiErrorBody),
        (status = 404, description = "Account not found", body = ApiErrorBody)
    )
)]
pub async fn upload_account_logo(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Json<AccountOverview>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let mut account = load_account(state.as_ref(), &account_id)?;

    if bytes.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidAccountLogo,
            "account logo file is empty",
        ));
    }
    if bytes.len() > MAX_ACCOUNT_LOGO_BYTES {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidAccountLogo,
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
#[utoipa::path(
    get,
    path = "/v1/account-assets/logos/{image_id}",
    tag = "accounts",
    summary = "Get account logo",
    description = "Returns the stored account logo image bytes.",
    params(("image_id" = String, Path, description = "Logo image identifier")),
    responses(
        (status = 200, description = "Logo image bytes", content_type = "image/*", body = [u8]),
        (status = 404, description = "Logo not found", body = ApiErrorBody)
    )
)]
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
        ApiErrorCode::NotFound,
        "account logo not found",
    ))
}

/// DELETE /v1/accounts/{account_id}
///
/// Removes the managed OS keyring secret, stops the supervisor runtime,
/// deletes the config file, and emits an `account.deleted` event.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[utoipa::path(
    delete,
    path = "/v1/accounts/{account_id}",
    tag = "accounts",
    summary = "Delete account",
    description = "Removes the managed keyring secret, stops the runtime, deletes config, and \
                   emits an account.deleted event.",
    params(("account_id" = String, Path, description = "Account identifier")),
    responses(
        (status = 200, description = "Account deleted", body = OkResponse),
        (status = 404, description = "Account not found", body = ApiErrorBody)
    )
)]
pub async fn delete_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let account = load_account(state.as_ref(), &account_id)?;
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
#[utoipa::path(
    post,
    path = "/v1/config:reload",
    tag = "sync",
    summary = "Reload configuration",
    description = "Re-reads config from disk, diffs against the in-memory snapshot, and \
                   starts/stops runtimes for changed accounts.",
    responses(
        (status = 200, description = "Configuration reloaded", body = OkResponse),
        (status = 400, description = "Configuration invalid", body = ApiErrorBody),
        (status = 500, description = "Configuration reload failed", body = ApiErrorBody)
    )
)]
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

    let mut resources = vec![ResourceChange::config_reloaded()];
    resources.extend(
        diff.added_sources
            .iter()
            .map(|id| ResourceChange::account(ResourceOperation::Created, id)),
    );
    resources.extend(
        diff.changed_sources
            .iter()
            .map(|id| ResourceChange::account(ResourceOperation::Updated, id)),
    );
    resources.extend(
        diff.removed_sources
            .iter()
            .map(|id| ResourceChange::account(ResourceOperation::Deleted, id)),
    );
    append_and_publish_config_event(
        &state,
        EVENT_TOPIC_CONFIG_RELOADED,
        resources,
        json!({
            "addedSourceCount": diff.added_sources.len(),
            "changedSourceCount": diff.changed_sources.len(),
            "removedSourceCount": diff.removed_sources.len(),
        }),
    )
    .map_err(store_error_to_api)?;

    Ok(Json(OkResponse { ok: true }))
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

/// Toggle the `enabled` flag on an account, re-persist, and restart the supervisor.
///
/// @spec docs/L1-api#account-crud-lifecycle
async fn set_account_enabled(
    state: Arc<AppState>,
    account_id: String,
    enabled: bool,
) -> Result<Json<OkResponse>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let mut account = load_account(state.as_ref(), &account_id)?;
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
            ApiErrorCode::InvalidAccountLogo,
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

// ---- Capability-token minting (`POST /v1/auth/tokens`) ----

/// Request body for `POST /v1/auth/tokens`: the scope a derived capability token
/// should carry. Every field NARROWS authority — the minted token is the
/// caller's own token with these caveats appended (attenuation), so it can never
/// exceed what the caller already holds. All fields are optional; an empty
/// request returns a token equivalent in authority to the caller's.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAuthTokenRequest {
    /// Restrict the token to these actions (subset of
    /// `read,send,tag,move,delete,manage`). Omitted = no added action caveat.
    pub actions: Option<Vec<Action>>,
    /// Restrict the token to a single account (`source_id`).
    pub account: Option<String>,
    /// Restrict the token to a single mailbox.
    pub mailbox: Option<String>,
    /// Restrict the token to a single message.
    pub message: Option<String>,
    /// Token lifetime in seconds from now. Omitted = no expiry caveat (lives as
    /// long as the root key). Recommended for shared/agent tokens.
    pub expires_in_seconds: Option<u64>,
}

/// Response for `POST /v1/auth/tokens`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAuthTokenResponse {
    /// The minted capability token (a macaroon), for use as
    /// `Authorization: Bearer <token>`.
    pub token: String,
    /// RFC3339 UTC expiry, present iff `expiresInSeconds` was set.
    pub expires_at: Option<String>,
}

/// The caveat value to use for a resource axis: the field verbatim if it has
/// non-whitespace content, else `None`. NOT trimmed — the caveat is compared for
/// exact equality against the request's path value, so the value must match what
/// the client will send on the path.
fn caveat_value(field: &Option<String>) -> Option<&str> {
    field.as_deref().filter(|value| !value.trim().is_empty())
}

/// Translate a validated mint request into caveat predicate strings (the
/// documented `authz` format) plus the resolved RFC3339 expiry. Returns 400 on
/// an empty `actions` list or a zero/overflowing lifetime.
fn build_token_caveats(
    request: &CreateAuthTokenRequest,
    now: time::OffsetDateTime,
) -> Result<(Vec<String>, Option<String>), ApiError> {
    let bad_request = |message: &str| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidQuery,
            message.to_string(),
        )
    };

    let mut predicates = Vec::new();

    if let Some(actions) = &request.actions {
        if actions.is_empty() {
            return Err(bad_request("actions must not be empty when provided"));
        }
        let verbs = actions
            .iter()
            .map(|action| action.as_str())
            .collect::<Vec<_>>()
            .join(",");
        predicates.push(format!("action = {verbs}"));
    }
    if let Some(account) = caveat_value(&request.account) {
        predicates.push(format!("account = {account}"));
    }
    if let Some(mailbox) = caveat_value(&request.mailbox) {
        predicates.push(format!("mailbox = {mailbox}"));
    }
    if let Some(message) = caveat_value(&request.message) {
        predicates.push(format!("message = {message}"));
    }

    let expires_at = match request.expires_in_seconds {
        None => None,
        Some(0) => return Err(bad_request("expiresInSeconds must be greater than zero")),
        Some(seconds) => {
            let seconds =
                i64::try_from(seconds).map_err(|_| bad_request("expiresInSeconds is too large"))?;
            let expiry = now
                .checked_add(time::Duration::seconds(seconds))
                .ok_or_else(|| bad_request("expiresInSeconds is too large"))?;
            let formatted = expiry
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| internal_error("failed to format token expiry".to_string()))?;
            predicates.push(format!("expires = {formatted}"));
            Some(formatted)
        }
    };

    Ok((predicates, expires_at))
}

/// Mint a narrower capability token. The handler **attenuates the caller's own
/// token** (adding the requested caveats), so the result can only narrow — never
/// widen — the caller's authority, whatever scope is requested. The route is
/// `Manage`-gated with no resource axis, so only a full-scope (or unscoped
/// `manage`) token reaches here; resource-scoped tokens are rejected (403)
/// before the handler runs.
#[utoipa::path(
    post,
    path = "/v1/auth/tokens",
    tag = "auth",
    summary = "Mint a capability token",
    description = "Derives a narrower capability token from the caller's token by appending the \
requested caveats (attenuation). The minted token can only narrow the caller's authority, never \
widen it. Requires a full-scope (or unscoped `manage`) token.",
    request_body = CreateAuthTokenRequest,
    responses(
        (status = 200, description = "The minted capability token", body = CreateAuthTokenResponse),
        (status = 400, description = "Invalid scope request", body = ApiErrorBody),
        (status = 403, description = "Caller token is not authorized to mint", body = ApiErrorBody)
    )
)]
pub async fn create_auth_token(
    State(state): State<Arc<AppState>>,
    presented: Option<Extension<crate::auth::PresentedToken>>,
    Json(request): Json<CreateAuthTokenRequest>,
) -> Result<Json<CreateAuthTokenResponse>, ApiError> {
    let now = time::OffsetDateTime::now_utc();
    let (predicates, expires_at) = build_token_caveats(&request, now)?;
    let caller = presented.map(|Extension(crate::auth::PresentedToken(token))| token);
    let token = derive_capability_token(caller, &state.macaroon_root_key, &predicates)?;
    Ok(Json(CreateAuthTokenResponse { token, expires_at }))
}

/// Produce the minted token from the requested caveat predicates.
///
/// With a `caller` token (the normal, authenticated case) this **attenuates the
/// caller's own token**: attenuation can only ADD caveats, which AND together,
/// so the result is always ≤ the caller's authority — never wider, whatever was
/// requested. Without a caller (`require_auth` off, no token to preserve) it
/// mints from the root key with the requested caveats.
fn derive_capability_token(
    caller: Option<String>,
    root: &crate::token::RootKey,
    predicates: &[String],
) -> Result<String, ApiError> {
    match caller {
        Some(caller) => {
            let mut token = caller;
            for predicate in predicates {
                token = crate::token::attenuate(&token, predicate).map_err(|_| {
                    internal_error("failed to attenuate capability token".to_string())
                })?;
            }
            Ok(token)
        }
        None => {
            let refs: Vec<&str> = predicates.iter().map(String::as_str).collect();
            Ok(crate::token::mint_with_caveats(root, &refs))
        }
    }
}

#[cfg(test)]
mod tests;
