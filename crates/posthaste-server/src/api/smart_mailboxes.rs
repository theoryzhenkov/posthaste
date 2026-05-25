use super::*;

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
    append_and_publish_config_event(
        &state,
        EVENT_TOPIC_SMART_MAILBOX_CREATED,
        vec![resource(
            "smartMailbox",
            "created",
            Some(smart_mailbox.id.as_str()),
            None,
        )],
        json!({ "smartMailboxId": smart_mailbox.id.as_str() }),
    )
    .map_err(store_error_to_api)?;
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
    append_and_publish_config_event(
        &state,
        EVENT_TOPIC_SMART_MAILBOX_UPDATED,
        vec![resource(
            "smartMailbox",
            "updated",
            Some(smart_mailbox.id.as_str()),
            None,
        )],
        json!({ "smartMailboxId": smart_mailbox.id.as_str() }),
    )
    .map_err(store_error_to_api)?;
    Ok(Json(smart_mailbox))
}

/// DELETE /v1/smart-mailboxes/{id}
///
/// @spec docs/L1-api#smart-mailboxes
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
        vec![resource(
            "smartMailbox",
            "deleted",
            Some(smart_mailbox_id.as_str()),
            None,
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
        vec![resource("smartMailbox", "reset", None, None)],
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
            let search_rule = parse_optional_search_rule(Some(q))?.expect("non-empty query");
            let mailbox = state
                .service
                .get_smart_mailbox(&SmartMailboxId::from(smart_mailbox_id))
                .map_err(ApiError::from_service_error)?;
            let combined = combine_rules(vec![mailbox.rule, search_rule]);
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
