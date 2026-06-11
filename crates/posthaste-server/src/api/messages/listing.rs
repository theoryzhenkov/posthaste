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
