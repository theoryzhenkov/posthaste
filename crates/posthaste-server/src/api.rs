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
pub(crate) mod mailboxes;
pub mod message_commands;
pub(crate) mod messages;
pub(crate) mod read_calls;
mod search_support;
pub mod settings;
pub mod smart_mailboxes;
pub(crate) mod sync_events;

pub use accounts::{
    complete_account_oauth, create_account, delete_account, disable_account, enable_account,
    get_account, get_account_logo, list_accounts, patch_account, reload_config,
    start_account_oauth, start_provider_oauth, upload_account_logo, verify_account,
    AccountTransportRequest, CreateAccountRequest, OAuthCallbackQuery, PatchAccountRequest,
    SecretWriteMode, SecretWriteRequest, StartOAuthRequest, StartOAuthResponse,
    StartProviderOAuthRequest,
};
pub use auth_tokens::{create_auth_token, CreateAuthTokenRequest, CreateAuthTokenResponse};
pub use mailboxes::{list_mailboxes, patch_mailbox, PatchMailboxRequest};
pub use message_commands::{
    add_to_mailbox, destroy_message, remove_from_mailbox, replace_mailboxes, set_keywords,
};
pub use messages::{
    get_conversation, get_identity, get_message, get_message_attachment, get_reply_context,
    list_conversations, list_sender_addresses, list_source_messages, search_messages, send_message,
    ConversationPageResponse, GetAttachmentQuery, ListConversationsQuery,
    ListSmartMailboxMessagesQuery, ListSourceMessagesQuery, MessagePageResponse,
    SearchMessagesQuery,
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
pub use sync_events::{
    stream_events, trigger_sync, EventsQuery, TriggerSyncRequest, TriggerSyncResponse,
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
use messages::live_gateway;
use search_support::{
    combine_rules, parse_optional_search_rule, source_message_scope_rule,
    spawn_search_cache_visibility,
};

/// Product API readiness response.
///
/// @spec docs/L1-api#health
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: &'static str,
}

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

#[cfg(test)]
use accounts::{oauth_account_settings, oauth_provider_mail_transport};
#[cfg(test)]
use auth_tokens::{build_token_caveats, derive_capability_token};
#[cfg(test)]
use messages::validate_send_message_request;
#[cfg(test)]
use sync_events::is_live_event_after_backlog;

#[cfg(test)]
mod tests;
