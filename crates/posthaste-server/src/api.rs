use std::convert::Infallible;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::http::{HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use posthaste_domain::{
    now_iso8601 as domain_now_iso8601, AccountDriver, AccountId, AccountOverview, AccountSettings,
    AccountTransportOverview, AddToMailboxCommand, AppSettings, CommandResult, ConversationCursor,
    ConversationId, ConversationPage, ConversationSortField, ConversationSummary, ConversationView,
    DomainEvent, EventFilter, Identity, MailboxId, MailboxSummary, MessageAttachment,
    MessageDetail, MessageId, Recipient, RemoveFromMailboxCommand, ReplaceMailboxesCommand,
    ReplyContext, SecretKind, SecretRef, SecretStatus, SecretStorage, SendMessageRequest,
    ServiceError, SetKeywordsCommand, SidebarResponse, SmartMailbox, SmartMailboxId,
    SmartMailboxKind, SmartMailboxRule, SmartMailboxSummary, SortDirection,
    EVENT_TOPIC_ACCOUNT_CREATED, EVENT_TOPIC_ACCOUNT_DELETED, EVENT_TOPIC_ACCOUNT_UPDATED,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

use crate::{sanitize, AppState};

mod account_support;
mod cursor_support;

use account_support::{
    account_overview, append_and_publish_account_event, apply_account_patch,
    apply_secret_instruction, delete_managed_secret, generate_smart_mailbox_id, internal_error,
    store_error_to_api, validate_account_settings,
};
use cursor_support::{
    conversation_limit, conversation_page_response, event_to_sse, matches_event,
    parse_conversation_cursor,
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

/// Request body for `PATCH /v1/settings`.
///
/// @spec docs/L1-api#settings
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchSettingsRequest {
    pub default_account_id: Option<String>,
}

/// Transport fields for account create/patch requests.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountTransportRequest {
    pub base_url: Option<String>,
    pub username: Option<String>,
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

/// Request body for `POST /v1/accounts`.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountRequest {
    pub id: String,
    pub name: String,
    pub driver: AccountDriver,
    pub enabled: Option<bool>,
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
    pub driver: Option<AccountDriver>,
    pub enabled: Option<bool>,
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
    if let Some(default_account_id) = &request.default_account_id {
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
    }

    let settings = AppSettings {
        default_account_id: request.default_account_id.map(AccountId),
    };
    state
        .service
        .put_app_settings(&settings)
        .map_err(ApiError::from_service_error)?;
    Ok(Json(settings))
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
        driver,
        enabled,
        transport,
        secret,
    } = request;
    let account_id = AccountId::from(id.as_str());
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
        name,
        driver,
        enabled: enabled.unwrap_or(true),
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
    delete_managed_secret(state.as_ref(), account.transport.secret_ref.as_ref())?;
    state.supervisor.remove_account(&account_id).await;
    state
        .service
        .delete_source(&account_id)
        .map_err(ApiError::from_service_error)?;
    append_and_publish_account_event(&state, &account_id, EVENT_TOPIC_ACCOUNT_DELETED)
        .map_err(store_error_to_api)?;
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

/// GET /v1/sources/{source_id}/messages
///
/// @spec docs/L1-api#conversations-and-messages
pub async fn list_source_messages(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    Query(query): Query<ListSourceMessagesQuery>,
) -> Result<Json<Vec<posthaste_domain::MessageSummary>>, ApiError> {
    let mailbox_id = query.mailbox_id.map(MailboxId);
    state
        .service
        .list_messages(&AccountId(source_id), mailbox_id.as_ref())
        .map(Json)
        .map_err(ApiError::from_service_error)
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
) -> Result<Json<Vec<posthaste_domain::MessageSummary>>, ApiError> {
    state
        .service
        .list_smart_mailbox_messages(&SmartMailboxId::from(smart_mailbox_id))
        .map(Json)
        .map_err(ApiError::from_service_error)
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
            let search_rule = posthaste_domain::search::parse_query(q)
                .map_err(|msg| ApiError::new(StatusCode::BAD_REQUEST, "invalid_query", msg))?;
            let mailbox = state
                .service
                .get_smart_mailbox(&SmartMailboxId::from(smart_mailbox_id))
                .map_err(ApiError::from_service_error)?;
            let combined = SmartMailboxRule {
                root: posthaste_domain::SmartMailboxGroup {
                    operator: posthaste_domain::SmartMailboxGroupOperator::All,
                    negated: false,
                    nodes: vec![
                        posthaste_domain::SmartMailboxRuleNode::Group(mailbox.rule.root),
                        posthaste_domain::SmartMailboxRuleNode::Group(search_rule.root),
                    ],
                },
            };
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
            let rule = posthaste_domain::search::parse_query(q)
                .map_err(|msg| ApiError::new(StatusCode::BAD_REQUEST, "invalid_query", msg))?;
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
    let result = state
        .service
        .get_message_detail(&account_id, &message_id_ref)
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
    let result = state
        .service
        .get_message_detail(&account_id, &message_id)
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
    let bytes = state
        .service
        .download_blob(&account_id, &attachment.blob_id)
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
    state
        .service
        .fetch_identity(&AccountId(source_id))
        .await
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// GET /v1/sources/{source_id}/messages/{id}/reply-context
///
/// @spec docs/L1-api#compose
/// @spec docs/L1-compose#reply-quoting
pub async fn get_reply_context(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
) -> Result<Json<ReplyContext>, ApiError> {
    state
        .service
        .fetch_reply_context(&AccountId(source_id), &MessageId(message_id))
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
    state
        .service
        .send_message(&AccountId(source_id), &request)
        .await
        .map_err(ApiError::from_service_error)?;
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

fn validate_send_message_request(request: &SendMessageRequest) -> Result<(), ApiError> {
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

/// POST /v1/sources/{sid}/commands/messages/{mid}/set-keywords
///
/// @spec docs/L1-api#message-commands
pub async fn set_keywords(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
    Json(command): Json<SetKeywordsCommand>,
) -> Result<Json<CommandResult>, ApiError> {
    let result = state
        .service
        .set_keywords(&AccountId(source_id), &MessageId(message_id), &command)
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
    let result = state
        .service
        .add_to_mailbox(&AccountId(source_id), &MessageId(message_id), &command)
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
    let result = state
        .service
        .remove_from_mailbox(&AccountId(source_id), &MessageId(message_id), &command)
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
    let result = state
        .service
        .replace_mailboxes(&AccountId(source_id), &MessageId(message_id), &command)
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
    let result = state
        .service
        .destroy_message(&AccountId(source_id), &MessageId(message_id))
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

#[cfg(test)]
mod tests {
    use super::*;
    use account_support::{secret_status, validate_secret_request};
    use cursor_support::encode_conversation_cursor;
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
            driver: AccountDriver::Jmap,
            enabled: true,
            transport: posthaste_domain::AccountTransportSettings {
                base_url: Some("https://example.com/jmap".to_string()),
                username: Some("alice@example.com".to_string()),
                secret_ref: None,
            },
            created_at: "2026-03-31T10:00:00Z".to_string(),
            updated_at: "2026-03-31T10:00:00Z".to_string(),
        };

        let error = validate_account_settings(&account).expect_err("validation should fail");

        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.body.code, "invalid_account");
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
            driver: AccountDriver::Jmap,
            enabled: true,
            transport: posthaste_domain::AccountTransportSettings {
                base_url: Some("https://before.example/jmap".to_string()),
                username: Some("alice@example.com".to_string()),
                secret_ref: None,
            },
            created_at: "2026-03-31T10:00:00Z".to_string(),
            updated_at: "2026-03-31T10:00:00Z".to_string(),
        };

        apply_account_patch(
            &mut account,
            &PatchAccountRequest {
                name: None,
                driver: None,
                enabled: None,
                transport: Some(AccountTransportRequest {
                    base_url: Some("https://after.example/jmap".to_string()),
                    username: None,
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
}
