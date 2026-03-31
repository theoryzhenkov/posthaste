use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use mail_domain::{
    AccountId, AddToMailboxCommand, CommandResult, DomainEvent, EventFilter, MailboxId,
    MailboxSummary, MessageId, RemoveFromMailboxCommand, ReplaceMailboxesCommand, ServiceError,
    SetKeywordsCommand, ThreadId,
};
use serde::Deserialize;
use serde_json::json;
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
    pub account_id: String,
    pub topic: Option<String>,
    pub mailbox_id: Option<String>,
    pub after_seq: Option<i64>,
}

#[derive(Debug, serde::Serialize)]
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

impl ApiError {
    pub fn from_service_error(error: ServiceError) -> Self {
        let status = match error.code() {
            "not_found" => StatusCode::NOT_FOUND,
            "conflict" | "state_mismatch" => StatusCode::CONFLICT,
            "auth_error" => StatusCode::UNAUTHORIZED,
            "gateway_unavailable" => StatusCode::SERVICE_UNAVAILABLE,
            "network_error" => StatusCode::BAD_GATEWAY,
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
    let events = state
        .service
        .sync_account(&account_id)
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&events);
    Ok(Json(json!({ "ok": true, "eventCount": events.len() })))
}

pub async fn stream_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let filter = EventFilter {
        account_id: AccountId(query.account_id),
        topic: query.topic,
        mailbox_id: query.mailbox_id.map(MailboxId),
        after_seq: query.after_seq,
    };
    let backlog = state
        .service
        .list_events(&filter)
        .map_err(ApiError::from_service_error)?;
    let receiver = state.event_sender.subscribe();
    let backlog_filter = EventFilter {
        account_id: filter.account_id.clone(),
        topic: filter.topic.clone(),
        mailbox_id: filter.mailbox_id.clone(),
        after_seq: filter.after_seq,
    };
    let backlog_stream = tokio_stream::iter(
        backlog
            .into_iter()
            .filter(move |event| matches_event(event, &backlog_filter))
            .map(event_to_sse),
    );
    let live_filter = EventFilter {
        account_id: filter.account_id.clone(),
        topic: filter.topic.clone(),
        mailbox_id: filter.mailbox_id.clone(),
        after_seq: filter.after_seq,
    };
    let live_stream = BroadcastStream::new(receiver).filter_map(move |message| {
        let live_filter = EventFilter {
            account_id: live_filter.account_id.clone(),
            topic: live_filter.topic.clone(),
            mailbox_id: live_filter.mailbox_id.clone(),
            after_seq: live_filter.after_seq,
        };
        match message {
            Ok(event) if matches_event(&event, &live_filter) => Some(event_to_sse(event)),
            _ => None,
        }
    });
    Ok(Sse::new(backlog_stream.chain(live_stream)).keep_alive(KeepAlive::default()))
}

fn matches_event(event: &DomainEvent, filter: &EventFilter) -> bool {
    if event.account_id != filter.account_id {
        return false;
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
        .event(event.topic.as_str())
        .id(event.seq.to_string())
        .json_data(event)
        .unwrap_or_else(|_| Event::default().data("{}")))
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
            account_id: AccountId::from("primary"),
            topic: Some("message.arrived".to_string()),
            mailbox_id: Some(MailboxId::from("inbox")),
            after_seq: Some(4),
        };
        let wrong_mailbox_filter = EventFilter {
            account_id: AccountId::from("primary"),
            topic: Some("message.arrived".to_string()),
            mailbox_id: Some(MailboxId::from("archive")),
            after_seq: Some(4),
        };

        assert!(matches_event(&event, &matching_filter));
        assert!(!matches_event(&event, &wrong_mailbox_filter));
        assert!(!matches_event(
            &event,
            &EventFilter {
                account_id: AccountId::from("secondary"),
                topic: matching_filter.topic.clone(),
                mailbox_id: matching_filter.mailbox_id.clone(),
                after_seq: matching_filter.after_seq,
            }
        ));
        assert!(!matches_event(
            &event,
            &EventFilter {
                account_id: AccountId::from("primary"),
                topic: Some("message.updated".to_string()),
                mailbox_id: Some(MailboxId::from("inbox")),
                after_seq: Some(4),
            }
        ));
        assert!(!matches_event(
            &event,
            &EventFilter {
                account_id: AccountId::from("primary"),
                topic: Some("message.arrived".to_string()),
                mailbox_id: Some(MailboxId::from("inbox")),
                after_seq: Some(5),
            }
        ));
    }

    #[test]
    fn api_error_maps_state_mismatch_to_conflict() {
        let error = ApiError::from_service_error(ServiceError::from(
            mail_domain::GatewayError::StateMismatch,
        ));

        assert_eq!(error.status, StatusCode::CONFLICT);
        assert_eq!(error.body.code, "state_mismatch");
    }
}
