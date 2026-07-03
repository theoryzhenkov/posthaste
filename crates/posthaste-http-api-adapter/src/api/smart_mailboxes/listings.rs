use super::*;

/// GET /v1/smart-mailboxes/{id}/messages
///
/// @spec docs/L1-api#smart-mailboxes
#[utoipa::path(
    get,
    path = "/v1/smart-mailboxes/{smart_mailbox_id}/messages",
    tag = "smart-mailboxes",
    summary = "List smart mailbox messages",
    description = "Returns a paginated page of message summaries matching a smart mailbox query, \
                   optionally narrowed by a search query.",
    params(
        ("smart_mailbox_id" = String, Path, description = "Smart mailbox identifier"),
        ListSmartMailboxMessagesQuery
    ),
    responses(
        (status = 200, description = "A page of message summaries", body = MessagePageResponse),
        (status = 400, description = "Invalid cursor or query", body = ApiErrorBody),
        (status = 404, description = "Smart mailbox not found", body = ApiErrorBody)
    )
)]
pub async fn list_smart_mailbox_messages(
    State(state): State<Arc<AppState>>,
    Path(smart_mailbox_id): Path<String>,
    headers: HeaderMap,
    Query(query): Query<ListSmartMailboxMessagesQuery>,
) -> Result<Json<MessagePageResponse>, ApiError> {
    let limit = message_limit(query.limit)?;
    let cursor = parse_message_cursor(query.cursor.as_deref())?;
    let sort_field = query.sort.unwrap_or_default();
    let sort_direction = query.sort_dir.unwrap_or_default();
    let smart_mailbox_id = SmartMailboxId::from(smart_mailbox_id);
    let base_query = smart_mailbox_query(&smart_mailbox_id);
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
    let as_of_seq = state.runtime.current_event_seq().await;
    Ok(Json(message_page_response(page, as_of_seq)))
}

/// GET /v1/smart-mailboxes/{id}/conversations
///
/// @spec docs/L1-api#smart-mailboxes
/// @spec docs/L1-api#cursor-pagination
#[utoipa::path(
    get,
    path = "/v1/smart-mailboxes/{smart_mailbox_id}/conversations",
    tag = "smart-mailboxes",
    summary = "List smart mailbox conversations",
    description = "Returns a paginated page of conversation-grouped rows matching a smart mailbox \
                   query, optionally narrowed by a search query.",
    params(
        ("smart_mailbox_id" = String, Path, description = "Smart mailbox identifier"),
        ListConversationsQuery
    ),
    responses(
        (status = 200, description = "A page of conversation summaries", body = ConversationPageResponse),
        (status = 400, description = "Invalid cursor or query", body = ApiErrorBody),
        (status = 404, description = "Smart mailbox not found", body = ApiErrorBody)
    )
)]
pub async fn list_smart_mailbox_conversations(
    State(state): State<Arc<AppState>>,
    Path(smart_mailbox_id): Path<String>,
    Query(query): Query<ListConversationsQuery>,
) -> Result<Json<ConversationPageResponse>, ApiError> {
    let limit = conversation_limit(query.limit)?;
    let cursor = parse_conversation_cursor(query.cursor.as_deref())?;
    let sort_field = query.sort.unwrap_or_default();
    let sort_direction = query.sort_dir.unwrap_or_default();
    let smart_mailbox_id = SmartMailboxId::from(smart_mailbox_id);
    let smart_query = smart_mailbox_query(&smart_mailbox_id);
    let source_id = query.source_id.as_deref().map(AccountId::from);
    let mailbox_id = query.mailbox_id.as_deref().map(MailboxId::from);
    let source_query = match (source_id.as_ref(), mailbox_id.as_ref()) {
        (Some(account_id), Some(mailbox_id)) => Some(mailbox_query(account_id, mailbox_id)),
        (Some(account_id), None) => Some(account_query(account_id)),
        (None, _) => None,
    };
    let query_text = join_query([
        Some(smart_query),
        source_query,
        optional_user_query(query.q.as_deref()),
    ]);
    let page = state
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
        .and_then(expect_conversation_page)?;
    let as_of_seq = state.runtime.current_event_seq().await;
    Ok(Json(conversation_page_response(page, as_of_seq)))
}
