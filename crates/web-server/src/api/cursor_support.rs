use std::convert::Infallible;

use axum::response::sse::Event;

use super::*;

const DEFAULT_CONVERSATION_LIMIT: usize = 100;
const MAX_CONVERSATION_LIMIT: usize = 250;

pub(super) fn matches_event(event: &DomainEvent, filter: &EventFilter) -> bool {
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

pub(super) fn event_to_sse(event: DomainEvent) -> Result<Event, Infallible> {
    Ok(Event::default()
        .id(event.seq.to_string())
        .json_data(event)
        .unwrap_or_else(|_| Event::default().data("{}")))
}

pub(super) fn conversation_limit(limit: Option<usize>) -> Result<usize, ApiError> {
    let limit = limit.unwrap_or(DEFAULT_CONVERSATION_LIMIT);
    if limit == 0 || limit > MAX_CONVERSATION_LIMIT {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_limit",
            format!("limit must be between 1 and {MAX_CONVERSATION_LIMIT} conversations"),
        ));
    }
    Ok(limit)
}

pub(super) fn parse_conversation_cursor(
    cursor: Option<&str>,
) -> Result<Option<ConversationCursor>, ApiError> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let Some((len_prefix, remainder)) = cursor.split_once(':') else {
        return Err(invalid_cursor());
    };
    let timestamp_len = len_prefix.parse::<usize>().map_err(|_| invalid_cursor())?;
    if timestamp_len == 0 || remainder.len() <= timestamp_len {
        return Err(invalid_cursor());
    }
    let (latest_received_at, conversation_id) = remainder.split_at(timestamp_len);
    let Some(conversation_id) = conversation_id.strip_prefix(':') else {
        return Err(invalid_cursor());
    };
    if latest_received_at.is_empty() || conversation_id.is_empty() {
        return Err(invalid_cursor());
    }
    Ok(Some(ConversationCursor {
        latest_received_at: latest_received_at.to_string(),
        conversation_id: ConversationId::from(conversation_id),
    }))
}

pub(super) fn invalid_cursor() -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_cursor",
        "cursor must include a timestamp and conversation id",
    )
}

pub(super) fn encode_conversation_cursor(cursor: &ConversationCursor) -> String {
    format!(
        "{}:{}:{}",
        cursor.latest_received_at.len(),
        cursor.latest_received_at,
        cursor.conversation_id.as_str()
    )
}

pub(super) fn conversation_page_response(page: ConversationPage) -> ConversationPageResponse {
    ConversationPageResponse {
        items: page.items,
        next_cursor: page.next_cursor.as_ref().map(encode_conversation_cursor),
    }
}
