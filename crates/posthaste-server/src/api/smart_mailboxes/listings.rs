use super::*;

/// GET /v1/smart-mailboxes/{id}/messages
///
/// @spec docs/L1-api#smart-mailboxes
#[utoipa::path(
    get,
    path = "/v1/smart-mailboxes/{smart_mailbox_id}/messages",
    tag = "smart-mailboxes",
    summary = "List smart mailbox messages",
    description = "Returns a paginated page of message summaries matching a smart mailbox rule, \
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
    if let Some(search_rule) = parse_optional_search_rule(query.q.as_deref())? {
        let mailbox = state
            .service
            .get_smart_mailbox(&smart_mailbox_id)
            .map_err(ApiError::from_service_error)?;
        let scope_rule = mailbox.rule;
        let result_rule = combine_rules(vec![scope_rule.clone(), search_rule]);
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
            scope_rule,
            result_rule,
            operation_id,
        );
        return Ok(Json(message_page_response(page)));
    }

    let page = state
        .service
        .list_smart_mailbox_message_page(
            &smart_mailbox_id,
            limit,
            cursor.as_ref(),
            sort_field,
            sort_direction,
        )
        .map_err(ApiError::from_service_error)?;
    Ok(Json(message_page_response(page)))
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
    description = "Returns a paginated page of conversation summaries matching a smart mailbox \
                   rule, optionally narrowed by a search query.",
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
    let search_rule = parse_optional_search_rule(query.q.as_deref())?;
    // A `sourceId` (optionally `mailboxId`) filter becomes an account scope rule.
    //
    // SECURITY: the smart-mailbox rule alone can span accounts, so an
    // account-scoped capability token (which the auth layer requires to carry a
    // matching `?sourceId`) needs this scope ANDed in — in BOTH the search and
    // non-search branches — for the route's `account` Filter to mean anything.
    // The previous code dropped `sourceId` entirely, which is why the route was
    // mapped with no resource axis. Do not drop it.
    let source_scope = query.source_id.as_deref().map(|source_id| {
        let mailbox_id = query.mailbox_id.as_deref().map(MailboxId::from);
        source_message_scope_rule(source_id, mailbox_id.as_ref())
    });

    // When a search query OR a source scope is present, build the combined rule
    // explicitly so the scope is enforced; otherwise use the plain listing.
    if search_rule.is_some() || source_scope.is_some() {
        let mailbox = state
            .service
            .get_smart_mailbox(&smart_mailbox_id)
            .map_err(ApiError::from_service_error)?;
        let mut rules = vec![mailbox.rule];
        rules.extend(source_scope);
        rules.extend(search_rule);
        let combined = combine_rules(rules);
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

    state
        .service
        .list_smart_mailbox_conversations(
            &smart_mailbox_id,
            limit,
            cursor.as_ref(),
            sort_field,
            sort_direction,
        )
        .map(conversation_page_response)
        .map(Json)
        .map_err(ApiError::from_service_error)
}
