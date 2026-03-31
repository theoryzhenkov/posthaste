use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use mail_domain::{
    AccountDriver, AccountId, AccountOverview, AccountSettings, AccountTransportOverview,
    AddToMailboxCommand, AppSettings, CommandResult, DomainEvent, EventFilter, GatewayError,
    MailboxId, MailboxSummary, MessageId, RemoveFromMailboxCommand, ReplaceMailboxesCommand,
    SecretKind, SecretRef, SecretStatus, SecretStorage, ServiceError, SetKeywordsCommand,
    ThreadId,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

use crate::{sanitize, AppState};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMessagesQuery {
    pub mailbox_id: Option<String>,
}

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
pub struct PatchSettingsRequest {
    pub default_account_id: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountTransportRequest {
    pub base_url: Option<String>,
    pub username: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SecretWriteMode {
    #[default]
    Keep,
    Replace,
    Clear,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretWriteRequest {
    #[serde(default)]
    pub mode: SecretWriteMode,
    pub password: Option<String>,
}

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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchAccountRequest {
    pub name: Option<String>,
    pub driver: Option<AccountDriver>,
    pub enabled: Option<bool>,
    pub transport: Option<AccountTransportRequest>,
    pub secret: Option<SecretWriteRequest>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
    pub details: serde_json::Value,
}

pub struct ApiError {
    status: StatusCode,
    body: ApiErrorBody,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResponse {
    pub ok: bool,
    pub identity_email: Option<String>,
    pub push_supported: bool,
}

impl ApiError {
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

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

pub async fn get_settings(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AppSettings>, ApiError> {
    state
        .store
        .get_app_settings()
        .map(Json)
        .map_err(store_error_to_api)
}

pub async fn patch_settings(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PatchSettingsRequest>,
) -> Result<Json<AppSettings>, ApiError> {
    if let Some(default_account_id) = &request.default_account_id {
        let account = state
            .store
            .get_account(&AccountId::from(default_account_id.as_str()))
            .map_err(store_error_to_api)?;
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
        .store
        .put_app_settings(&settings)
        .map_err(store_error_to_api)?;
    Ok(Json(settings))
}

pub async fn list_accounts(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<AccountOverview>>, ApiError> {
    let settings = state.store.get_app_settings().map_err(store_error_to_api)?;
    let accounts = state.store.list_accounts().map_err(store_error_to_api)?;
    let mut response = Vec::with_capacity(accounts.len());
    for account in accounts {
        response.push(account_overview(&state, &settings, account).await);
    }
    Ok(Json(response))
}

pub async fn get_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<AccountOverview>, ApiError> {
    let settings = state.store.get_app_settings().map_err(store_error_to_api)?;
    let account = state
        .store
        .get_account(&AccountId::from(account_id.as_str()))
        .map_err(store_error_to_api)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "account not found"))?;
    Ok(Json(account_overview(&state, &settings, account).await))
}

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
        .store
        .get_account(&account_id)
        .map_err(store_error_to_api)?
        .is_some()
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "conflict",
            "account already exists",
        ));
    }

    let timestamp = now_iso8601().map_err(internal_error)?;
    let mut account = AccountSettings {
        id: account_id.clone(),
        name,
        driver,
        enabled: enabled.unwrap_or(true),
        transport: transport.into(),
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };
    apply_secret_instruction(
        state.as_ref(),
        &mut account,
        None,
        &secret,
    )?;
    validate_account_settings(&account)?;
    state
        .store
        .create_account(&account)
        .map_err(store_error_to_api)?;
    state.supervisor.start_account(&account).await;
    append_and_publish_account_event(&state, &account_id, "account.created")
        .map_err(store_error_to_api)?;

    let settings = state.store.get_app_settings().map_err(store_error_to_api)?;
    Ok(Json(account_overview(&state, &settings, account).await))
}

pub async fn patch_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
    Json(request): Json<PatchAccountRequest>,
) -> Result<Json<AccountOverview>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let mut account = state
        .store
        .get_account(&account_id)
        .map_err(store_error_to_api)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "account not found"))?;
    if let Some(name) = request.name {
        account.name = name;
    }
    if let Some(driver) = request.driver {
        account.driver = driver;
    }
    if let Some(enabled) = request.enabled {
        account.enabled = enabled;
    }
    if let Some(transport) = request.transport {
        account.transport.base_url = normalize_optional(transport.base_url);
        account.transport.username = normalize_optional(transport.username);
    }
    account.updated_at = now_iso8601().map_err(internal_error)?;
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
        .store
        .update_account(&account)
        .map_err(store_error_to_api)?;
    state.supervisor.start_account(&account).await;
    append_and_publish_account_event(&state, &account_id, "account.updated")
        .map_err(store_error_to_api)?;

    let settings = state.store.get_app_settings().map_err(store_error_to_api)?;
    Ok(Json(account_overview(&state, &settings, account).await))
}

pub async fn verify_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<VerificationResponse>, ApiError> {
    let account = state
        .store
        .get_account(&AccountId::from(account_id.as_str()))
        .map_err(store_error_to_api)?
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

pub async fn enable_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    set_account_enabled(state, account_id, true).await
}

pub async fn disable_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    set_account_enabled(state, account_id, false).await
}

pub async fn delete_account(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let account = state
        .store
        .get_account(&account_id)
        .map_err(store_error_to_api)?;
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
        .store
        .delete_account(&account_id)
        .map_err(store_error_to_api)?;
    append_and_publish_account_event(&state, &account_id, "account.deleted")
        .map_err(store_error_to_api)?;
    Ok(Json(OkResponse { ok: true }))
}

pub async fn list_mailboxes(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<Vec<MailboxSummary>>, ApiError> {
    state
        .service
        .list_mailboxes(&AccountId(account_id))
        .map(Json)
        .map_err(ApiError::from_service_error)
}

pub async fn list_messages(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
    Query(query): Query<ListMessagesQuery>,
) -> Result<Json<Vec<mail_domain::MessageSummary>>, ApiError> {
    let mailbox_id = query.mailbox_id.map(MailboxId);
    state
        .service
        .list_messages(&AccountId(account_id), mailbox_id.as_ref())
        .map(Json)
        .map_err(ApiError::from_service_error)
}

pub async fn get_message(
    State(state): State<Arc<AppState>>,
    Path((account_id, message_id)): Path<(String, String)>,
) -> Result<Json<mail_domain::MessageDetail>, ApiError> {
    let result = state
        .service
        .get_message_detail(&AccountId(account_id), &MessageId(message_id))
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
        .map(|html| sanitize::sanitize_email_html(html));
    Ok(Json(detail))
}

pub async fn get_thread(
    State(state): State<Arc<AppState>>,
    Path((account_id, thread_id)): Path<(String, String)>,
) -> Result<Json<mail_domain::ThreadView>, ApiError> {
    state
        .service
        .get_thread(&AccountId(account_id), &ThreadId(thread_id))
        .map(Json)
        .map_err(ApiError::from_service_error)
}

pub async fn set_keywords(
    State(state): State<Arc<AppState>>,
    Path((account_id, message_id)): Path<(String, String)>,
    Json(command): Json<SetKeywordsCommand>,
) -> Result<Json<CommandResult>, ApiError> {
    let result = state
        .service
        .set_keywords(&AccountId(account_id), &MessageId(message_id), &command)
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&result.events);
    Ok(Json(result))
}

pub async fn add_to_mailbox(
    State(state): State<Arc<AppState>>,
    Path((account_id, message_id)): Path<(String, String)>,
    Json(command): Json<AddToMailboxCommand>,
) -> Result<Json<CommandResult>, ApiError> {
    let result = state
        .service
        .add_to_mailbox(&AccountId(account_id), &MessageId(message_id), &command)
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&result.events);
    Ok(Json(result))
}

pub async fn remove_from_mailbox(
    State(state): State<Arc<AppState>>,
    Path((account_id, message_id)): Path<(String, String)>,
    Json(command): Json<RemoveFromMailboxCommand>,
) -> Result<Json<CommandResult>, ApiError> {
    let result = state
        .service
        .remove_from_mailbox(&AccountId(account_id), &MessageId(message_id), &command)
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&result.events);
    Ok(Json(result))
}

pub async fn replace_mailboxes(
    State(state): State<Arc<AppState>>,
    Path((account_id, message_id)): Path<(String, String)>,
    Json(command): Json<ReplaceMailboxesCommand>,
) -> Result<Json<CommandResult>, ApiError> {
    let result = state
        .service
        .replace_mailboxes(&AccountId(account_id), &MessageId(message_id), &command)
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&result.events);
    Ok(Json(result))
}

pub async fn destroy_message(
    State(state): State<Arc<AppState>>,
    Path((account_id, message_id)): Path<(String, String)>,
) -> Result<Json<CommandResult>, ApiError> {
    let result = state
        .service
        .destroy_message(&AccountId(account_id), &MessageId(message_id))
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&result.events);
    Ok(Json(result))
}

pub async fn trigger_sync(
    State(state): State<Arc<AppState>>,
    Path(account_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let account_id = AccountId(account_id);
    let event_count = state
        .supervisor
        .sync_account(&account_id)
        .await
        .map_err(ApiError::from_service_error)?;
    Ok(Json(json!({ "ok": true, "eventCount": event_count })))
}

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
    let backlog = state
        .service
        .list_events(&filter)
        .map_err(ApiError::from_service_error)?;
    let receiver = state.event_sender.subscribe();
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
            Ok(event) if matches_event(&event, &live_filter) => Some(event_to_sse(event)),
            _ => None,
        }
    });
    Ok(Sse::new(backlog_stream.chain(live_stream)).keep_alive(KeepAlive::default()))
}

fn matches_event(event: &DomainEvent, filter: &EventFilter) -> bool {
    if let Some(account_id) = &filter.account_id {
        if &event.account_id != account_id {
            return false;
        }
    }
    if let Some(after_seq) = filter.after_seq {
        if event.seq <= after_seq {
            return false;
        }
    }
    if let Some(topic) = &filter.topic {
        if &event.topic != topic {
            return false;
        }
    }
    if let Some(mailbox_id) = &filter.mailbox_id {
        if event.mailbox_id.as_ref() != Some(mailbox_id) {
            return false;
        }
    }
    true
}

fn event_to_sse(event: DomainEvent) -> Result<Event, Infallible> {
    Ok(Event::default()
        .id(event.seq.to_string())
        .json_data(event)
        .unwrap_or_else(|_| Event::default().data("{}")))
}

async fn account_overview(
    state: &Arc<AppState>,
    settings: &AppSettings,
    account: AccountSettings,
) -> AccountOverview {
    let runtime = state.supervisor.runtime_overview(&account.id).await;
    AccountOverview {
        id: account.id.clone(),
        name: account.name.clone(),
        driver: account.driver.clone(),
        enabled: account.enabled,
        transport: account_transport_overview(&account),
        created_at: account.created_at.clone(),
        updated_at: account.updated_at.clone(),
        is_default: settings.default_account_id.as_ref() == Some(&account.id),
        runtime,
    }
}

fn account_transport_overview(account: &AccountSettings) -> AccountTransportOverview {
    AccountTransportOverview {
        base_url: account.transport.base_url.clone(),
        username: account.transport.username.clone(),
        secret: secret_status(account.transport.secret_ref.as_ref()),
    }
}

fn secret_status(secret_ref: Option<&SecretRef>) -> SecretStatus {
    match secret_ref {
        Some(secret_ref) => SecretStatus {
            storage: match secret_ref.kind {
                SecretKind::Env => SecretStorage::Env,
                SecretKind::Os => SecretStorage::Os,
            },
            configured: true,
            label: match secret_ref.kind {
                SecretKind::Env => Some(secret_ref.key.clone()),
                SecretKind::Os => None,
            },
        },
        None => SecretStatus {
            storage: SecretStorage::Os,
            configured: false,
            label: None,
        },
    }
}

impl From<AccountTransportRequest> for mail_domain::AccountTransportSettings {
    fn from(value: AccountTransportRequest) -> Self {
        Self {
            base_url: normalize_optional(value.base_url),
            username: normalize_optional(value.username),
            secret_ref: None,
        }
    }
}

fn apply_secret_instruction(
    state: &AppState,
    account: &mut AccountSettings,
    previous_secret_ref: Option<&SecretRef>,
    secret: &SecretWriteRequest,
) -> Result<(), ApiError> {
    validate_secret_request(secret)?;

    match secret.mode {
        SecretWriteMode::Keep => {
            if let Some(previous_secret_ref) = previous_secret_ref {
                account.transport.secret_ref = Some(previous_secret_ref.clone());
            }
        }
        SecretWriteMode::Replace => {
            let password = secret
                .password
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "invalid_secret",
                        "secret.password is required when secret.mode is replace",
                    )
                })?;
            let secret_ref = previous_secret_ref
                .filter(|secret_ref| matches!(secret_ref.kind, SecretKind::Os))
                .cloned()
                .unwrap_or_else(|| account_secret_ref(&account.id));
            match previous_secret_ref {
                Some(existing) if existing == &secret_ref => state
                    .secret_store
                    .update(&secret_ref, password)
                    .map_err(|error| ApiError::from_service_error(ServiceError::from(error)))?,
                _ => state
                    .secret_store
                    .save(&secret_ref, password)
                    .map_err(|error| ApiError::from_service_error(ServiceError::from(error)))?,
            }
            account.transport.secret_ref = Some(secret_ref);
        }
        SecretWriteMode::Clear => {
            delete_managed_secret(state, previous_secret_ref)?;
            account.transport.secret_ref = None;
        }
    }

    Ok(())
}

fn validate_secret_request(secret: &SecretWriteRequest) -> Result<(), ApiError> {
    match secret.mode {
        SecretWriteMode::Keep => {
            if secret.password.is_some() {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_secret",
                    "secret.password is only allowed when secret.mode is replace",
                ));
            }
        }
        SecretWriteMode::Replace => {
            if secret
                .password
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_secret",
                    "secret.password is required when secret.mode is replace",
                ));
            }
        }
        SecretWriteMode::Clear => {
            if secret.password.is_some() {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_secret",
                    "secret.password is not allowed when secret.mode is clear",
                ));
            }
        }
    }
    Ok(())
}

fn validate_account_settings(account: &AccountSettings) -> Result<(), ApiError> {
    if account.id.as_str().trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_account",
            "account id is required",
        ));
    }
    if account.name.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_account",
            "account name is required",
        ));
    }
    if matches!(account.driver, AccountDriver::Jmap) {
        if account
            .transport
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "JMAP base URL is required",
            ));
        }
        if account
            .transport
            .username
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "JMAP username is required",
            ));
        }
        if account.transport.secret_ref.is_none() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "invalid_account",
                "JMAP password must be configured before saving the account",
            ));
        }
    }
    Ok(())
}

fn account_secret_ref(account_id: &AccountId) -> SecretRef {
    SecretRef {
        kind: SecretKind::Os,
        key: format!("account:{}", account_id.as_str()),
    }
}

fn delete_managed_secret(
    state: &AppState,
    secret_ref: Option<&SecretRef>,
) -> Result<(), ApiError> {
    if let Some(secret_ref) = secret_ref {
        if matches!(secret_ref.kind, SecretKind::Os) {
            state
                .secret_store
                .delete(secret_ref)
                .map_err(|error| ApiError::from_service_error(ServiceError::from(error)))?;
        }
    }
    Ok(())
}

fn normalize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

async fn set_account_enabled(
    state: Arc<AppState>,
    account_id: String,
    enabled: bool,
) -> Result<Json<OkResponse>, ApiError> {
    let account_id = AccountId::from(account_id.as_str());
    let mut account = state
        .store
        .get_account(&account_id)
        .map_err(store_error_to_api)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "account not found"))?;
    account.enabled = enabled;
    account.updated_at = now_iso8601().map_err(internal_error)?;
    state
        .store
        .update_account(&account)
        .map_err(store_error_to_api)?;
    state.supervisor.start_account(&account).await;
    append_and_publish_account_event(
        &state,
        &account_id,
        if enabled {
            "account.updated"
        } else {
            "account.updated"
        },
    )
    .map_err(store_error_to_api)?;
    Ok(Json(OkResponse { ok: true }))
}

fn append_and_publish_account_event(
    state: &Arc<AppState>,
    account_id: &AccountId,
    topic: &str,
) -> Result<(), mail_domain::StoreError> {
    let event = state.store.append_event(
        account_id,
        topic,
        None,
        None,
        json!({ "accountId": account_id.as_str() }),
    )?;
    state.publish_events(&[event]);
    Ok(())
}

fn store_error_to_api(error: mail_domain::StoreError) -> ApiError {
    ApiError::from_service_error(ServiceError::from(error))
}

fn internal_error(error: String) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", error)
}

fn now_iso8601() -> Result<String, String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_event_applies_all_filters() {
        let event = DomainEvent {
            seq: 5,
            account_id: AccountId::from("primary"),
            topic: "message.arrived".to_string(),
            occurred_at: "2026-03-31T10:00:00Z".to_string(),
            mailbox_id: Some(MailboxId::from("inbox")),
            message_id: Some(MessageId::from("message-1")),
            payload: json!({"messageId": "message-1"}),
        };
        let matching_filter = EventFilter {
            account_id: Some(AccountId::from("primary")),
            topic: Some("message.arrived".to_string()),
            mailbox_id: Some(MailboxId::from("inbox")),
            after_seq: Some(4),
        };
        assert!(matches_event(&event, &matching_filter));
        assert!(matches_event(
            &event,
            &EventFilter {
                account_id: None,
                topic: Some("message.arrived".to_string()),
                mailbox_id: Some(MailboxId::from("inbox")),
                after_seq: Some(4),
            }
        ));
        assert!(!matches_event(
            &event,
            &EventFilter {
                account_id: Some(AccountId::from("secondary")),
                topic: Some("message.arrived".to_string()),
                mailbox_id: Some(MailboxId::from("inbox")),
                after_seq: Some(4),
            }
        ));
    }

    #[test]
    fn api_error_maps_state_mismatch_to_conflict() {
        let error = ApiError::from_service_error(ServiceError::from(GatewayError::StateMismatch));

        assert_eq!(error.status, StatusCode::CONFLICT);
        assert_eq!(error.body.code, "state_mismatch");
    }

    #[test]
    fn jmap_account_requires_configured_secret() {
        let account = AccountSettings {
            id: AccountId::from("primary"),
            name: "Primary".to_string(),
            driver: AccountDriver::Jmap,
            enabled: true,
            transport: mail_domain::AccountTransportSettings {
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
}
