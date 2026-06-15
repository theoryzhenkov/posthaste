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
use posthaste_runtime_contract::{
    AccountScopeRequest, AccountTransportMutation, CreateAccountMutation, PatchAccountMutation,
    RuntimeAccountList, RuntimeCaller, RuntimeCore, RuntimeError, RuntimeErrorCode,
    SecretWriteMode as RuntimeSecretWriteMode, SecretWriteMutation,
};
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
mod errors;
pub(crate) mod mailboxes;
pub mod message_commands;
pub(crate) mod messages;
pub(crate) mod read_calls;
mod search_support;
pub mod settings;
pub mod smart_mailboxes;
mod support;
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
pub use errors::{ApiError, ApiErrorBody, ApiErrorCode};
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
pub use settings::{
    get_settings, patch_settings, preview_automation_rule, AutomationRulePreviewResponse,
    PatchSettingsRequest, PreviewAutomationRuleRequest,
};
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
use support::{command_result_response, load_account};

/// Product API readiness response.
///
/// @spec docs/L1-api#health
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: &'static str,
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
