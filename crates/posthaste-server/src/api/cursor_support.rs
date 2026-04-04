use std::convert::Infallible;

use axum::response::sse::Event;

use super::*;

/// Default page size when no `limit` query parameter is provided.
///
/// @spec docs/L1-api#cursor-pagination
const DEFAULT_CONVERSATION_LIMIT: usize = 100;

/// Hard upper bound for the `limit` query parameter.
///
/// @spec docs/L1-api#cursor-pagination
const MAX_CONVERSATION_LIMIT: usize = 250;

/// Check whether a domain event passes the given filter criteria (account,
/// topic, mailbox, afterSeq).
///
/// @spec docs/L1-api#sse-event-stream
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

/// Convert a domain event into an SSE frame with `id` set to the sequence number.
///
/// @spec docs/L1-api#sse-event-stream
pub(super) fn event_to_sse(event: DomainEvent) -> Result<Event, Infallible> {
    Ok(Event::default()
        .id(event.seq.to_string())
        .json_data(event)
        .unwrap_or_else(|_| Event::default().data("{}")))
}

/// Resolve and validate the conversation page limit, defaulting to 100.
/// Returns 400 if `limit` is 0 or exceeds 250.
///
/// @spec docs/L1-api#cursor-pagination
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

/// Decode an opaque cursor string into a [`ConversationCursor`].
/// Format: `{value_len}:{sort_value}:{conversation_id}`.
///
/// @spec docs/L1-api#cursor-pagination
pub(super) fn parse_conversation_cursor(
    cursor: Option<&str>,
) -> Result<Option<ConversationCursor>, ApiError> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let Some((len_prefix, remainder)) = cursor.split_once(':') else {
        return Err(invalid_cursor());
    };
    let value_len = len_prefix.parse::<usize>().map_err(|_| invalid_cursor())?;
    if value_len == 0 || remainder.len() <= value_len {
        return Err(invalid_cursor());
    }
    let (sort_value, conversation_id) = remainder.split_at(value_len);
    let Some(conversation_id) = conversation_id.strip_prefix(':') else {
        return Err(invalid_cursor());
    };
    if sort_value.is_empty() || conversation_id.is_empty() {
        return Err(invalid_cursor());
    }
    Ok(Some(ConversationCursor {
        sort_value: sort_value.to_string(),
        conversation_id: ConversationId::from(conversation_id),
    }))
}

/// Construct a 400 error for a malformed conversation cursor.
pub(super) fn invalid_cursor() -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "invalid_cursor",
        "cursor must include a sort value and conversation id",
    )
}

/// Encode a [`ConversationCursor`] into its opaque string representation.
///
/// @spec docs/L1-api#cursor-pagination
pub(super) fn encode_conversation_cursor(cursor: &ConversationCursor) -> String {
    format!(
        "{}:{}:{}",
        cursor.sort_value.len(),
        cursor.sort_value,
        cursor.conversation_id.as_str()
    )
}

/// Convert a domain [`ConversationPage`] into the API response, encoding
/// the next cursor if more results exist.
///
/// @spec docs/L1-api#cursor-pagination
pub(super) fn conversation_page_response(page: ConversationPage) -> ConversationPageResponse {
    ConversationPageResponse {
        items: page.items,
        next_cursor: page.next_cursor.as_ref().map(encode_conversation_cursor),
    }
}
