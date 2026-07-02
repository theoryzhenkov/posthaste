use super::*;

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
    ensure_account_exists(state.as_ref(), &account_id).await?;
    validate_source_message_cursor(&account_id, cursor.as_ref())?;
    let sort_field = query.sort.unwrap_or_default();
    let sort_direction = query.sort_dir.unwrap_or_default();
    let base_query = mailbox_id
        .as_ref()
        .map(|mailbox_id| mailbox_query(&account_id, mailbox_id))
        .unwrap_or_else(|| account_query(&account_id));
    let query_text = join_query([
        Some(base_query.clone()),
        optional_user_query(query.q.as_deref()),
    ]);
    let visibility = visibility_for_search(
        base_query,
        query.q.as_deref(),
        observability::operation_id_from_headers(&headers),
    );
    let page = state
        .runtime
        .query_mail_page(
            RuntimeCaller::api(),
            MailQueryRequest {
                query: query_text,
                presentation: MailPresentationRequest::Messages {
                    limit: Some(limit),
                    cursor,
                    sort_field,
                    sort_direction,
                },
                visibility,
            },
        )
        .await
        .map_err(ApiError::from_runtime_error)
        .and_then(expect_message_page)?;
    Ok(Json(message_page_response(page)))
}

pub(super) fn validate_source_message_cursor(
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
    if query.q.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidQuery,
            "search query must not be empty",
        ));
    }
    state
        .runtime
        .query_mail_page(
            RuntimeCaller::api(),
            MailQueryRequest {
                query: query.q,
                presentation: MailPresentationRequest::Messages {
                    limit: Some(limit),
                    cursor,
                    sort_field,
                    sort_direction,
                },
                visibility: None,
            },
        )
        .await
        .map_err(ApiError::from_runtime_error)
        .and_then(expect_message_page)
        .map(message_page_response)
        .map(Json)
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
    let source_id = query.source_id.as_deref().map(AccountId::from);
    let mailbox_id = query.mailbox_id.as_deref().map(MailboxId::from);
    let base_query = match (source_id.as_ref(), mailbox_id.as_ref()) {
        (Some(account_id), Some(mailbox_id)) => Some(mailbox_query(account_id, mailbox_id)),
        (Some(account_id), None) => Some(account_query(account_id)),
        (None, _) => None,
    };
    let query_text = join_query([base_query, optional_user_query(query.q.as_deref())]);
    state
        .runtime
        .query_mail_page(
            RuntimeCaller::api(),
            MailQueryRequest {
                query: query_text,
                presentation: MailPresentationRequest::CollapsedByConversation {
                    limit,
                    cursor,
                    sort_field,
                    sort_direction,
                },
                visibility: None,
            },
        )
        .await
        .map_err(ApiError::from_runtime_error)
        .and_then(expect_conversation_page)
        .map(conversation_page_response)
        .map(Json)
}
