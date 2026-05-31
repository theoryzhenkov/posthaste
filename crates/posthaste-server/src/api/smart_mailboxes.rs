use super::*;

/// GET /v1/smart-mailboxes
///
/// @spec docs/L1-api#smart-mailboxes
#[utoipa::path(
    get,
    path = "/v1/smart-mailboxes",
    tag = "smart-mailboxes",
    summary = "List smart mailboxes",
    description = "Returns all smart mailboxes with live unread and total counts.",
    responses(
        (status = 200, description = "All smart mailboxes", body = [SmartMailboxSummary]),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
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
#[utoipa::path(
    post,
    path = "/v1/smart-mailboxes",
    tag = "smart-mailboxes",
    summary = "Create smart mailbox",
    description = "Generates an ID from the name and persists a new smart mailbox.",
    request_body = CreateSmartMailboxRequest,
    responses(
        (status = 200, description = "The created smart mailbox", body = SmartMailbox),
        (status = 400, description = "Validation failed", body = ApiErrorBody)
    )
)]
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
    append_and_publish_config_event(
        &state,
        EVENT_TOPIC_SMART_MAILBOX_CREATED,
        vec![ResourceChange::smart_mailbox(
            ResourceOperation::Created,
            &smart_mailbox.id,
        )],
        json!({ "smartMailboxId": smart_mailbox.id.as_str() }),
    )
    .map_err(store_error_to_api)?;
    Ok(Json(smart_mailbox))
}

/// GET /v1/smart-mailboxes/{id}
///
/// @spec docs/L1-api#smart-mailboxes
#[utoipa::path(
    get,
    path = "/v1/smart-mailboxes/{smart_mailbox_id}",
    tag = "smart-mailboxes",
    summary = "Get smart mailbox",
    description = "Returns a single smart mailbox with its rule.",
    params(("smart_mailbox_id" = String, Path, description = "Smart mailbox identifier")),
    responses(
        (status = 200, description = "The smart mailbox", body = SmartMailbox),
        (status = 404, description = "Smart mailbox not found", body = ApiErrorBody)
    )
)]
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
#[utoipa::path(
    patch,
    path = "/v1/smart-mailboxes/{smart_mailbox_id}",
    tag = "smart-mailboxes",
    summary = "Update smart mailbox",
    description = "Merges name, position, and rule fields. Omitted fields are preserved.",
    params(("smart_mailbox_id" = String, Path, description = "Smart mailbox identifier")),
    request_body = PatchSmartMailboxRequest,
    responses(
        (status = 200, description = "The updated smart mailbox", body = SmartMailbox),
        (status = 400, description = "Validation failed", body = ApiErrorBody),
        (status = 404, description = "Smart mailbox not found", body = ApiErrorBody)
    )
)]
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
    append_and_publish_config_event(
        &state,
        EVENT_TOPIC_SMART_MAILBOX_UPDATED,
        vec![ResourceChange::smart_mailbox(
            ResourceOperation::Updated,
            &smart_mailbox.id,
        )],
        json!({ "smartMailboxId": smart_mailbox.id.as_str() }),
    )
    .map_err(store_error_to_api)?;
    Ok(Json(smart_mailbox))
}

/// DELETE /v1/smart-mailboxes/{id}
///
/// @spec docs/L1-api#smart-mailboxes
#[utoipa::path(
    delete,
    path = "/v1/smart-mailboxes/{smart_mailbox_id}",
    tag = "smart-mailboxes",
    summary = "Delete smart mailbox",
    description = "Deletes a smart mailbox.",
    params(("smart_mailbox_id" = String, Path, description = "Smart mailbox identifier")),
    responses(
        (status = 200, description = "Smart mailbox deleted", body = OkResponse),
        (status = 404, description = "Smart mailbox not found", body = ApiErrorBody)
    )
)]
pub async fn delete_smart_mailbox(
    State(state): State<Arc<AppState>>,
    Path(smart_mailbox_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    let smart_mailbox_id = SmartMailboxId::from(smart_mailbox_id);
    state
        .service
        .delete_smart_mailbox(&smart_mailbox_id)
        .map_err(ApiError::from_service_error)?;
    append_and_publish_config_event(
        &state,
        EVENT_TOPIC_SMART_MAILBOX_DELETED,
        vec![ResourceChange::smart_mailbox(
            ResourceOperation::Deleted,
            &smart_mailbox_id,
        )],
        json!({ "smartMailboxId": smart_mailbox_id.as_str() }),
    )
    .map_err(store_error_to_api)?;
    Ok(Json(OkResponse { ok: true }))
}

/// POST /v1/smart-mailboxes:reset-defaults
///
/// Restores default smart mailboxes (Inbox, Archive, Drafts, Sent, Junk,
/// Trash, All Mail) and returns the full list.
///
/// @spec docs/L1-api#smart-mailbox-crud
/// @spec docs/L1-accounts#smart-mailbox-defaults
#[utoipa::path(
    post,
    path = "/v1/smart-mailboxes:reset-defaults",
    tag = "smart-mailboxes",
    summary = "Reset default smart mailboxes",
    description = "Restores the built-in default smart mailboxes and returns the full list.",
    responses(
        (status = 200, description = "All smart mailboxes after reset", body = [SmartMailboxSummary]),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn reset_default_smart_mailboxes(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SmartMailboxSummary>>, ApiError> {
    state
        .service
        .reset_default_smart_mailboxes()
        .map_err(ApiError::from_service_error)?;
    append_and_publish_config_event(
        &state,
        EVENT_TOPIC_SMART_MAILBOX_RESET,
        vec![ResourceChange::smart_mailbox_reset()],
        json!({ "scope": "smartMailboxes" }),
    )
    .map_err(store_error_to_api)?;
    state
        .service
        .list_smart_mailboxes()
        .map(Json)
        .map_err(ApiError::from_service_error)
}

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
