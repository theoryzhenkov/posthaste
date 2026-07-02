use super::*;

/// GET /v1/views/conversations/{id}
///
/// @spec docs/L1-api#conversations-and-messages
#[utoipa::path(
    get,
    path = "/v1/views/conversations/{conversation_id}",
    tag = "conversations",
    summary = "Get conversation",
    description = "Returns a full conversation with all messages expanded.",
    params(("conversation_id" = String, Path, description = "Conversation identifier")),
    responses(
        (status = 200, description = "The conversation detail", body = ConversationView),
        (status = 404, description = "Conversation not found", body = ApiErrorBody)
    )
)]
pub async fn get_conversation(
    State(state): State<Arc<AppState>>,
    Path(conversation_id): Path<String>,
) -> Result<Json<ConversationView>, ApiError> {
    let conversation_id = ConversationId::from(conversation_id);
    let page = state
        .runtime
        .query_mail_page(
            RuntimeCaller::api(),
            MailQueryRequest {
                query: format!("conversation:{}", conversation_id.as_str()),
                presentation: MailPresentationRequest::Messages {
                    limit: None,
                    cursor: None,
                    sort_field: MessageSortField::Date,
                    sort_direction: SortDirection::Asc,
                },
                visibility: None,
            },
        )
        .await
        .map_err(ApiError::from_runtime_error)
        .and_then(expect_message_page)?;
    if page.items.is_empty() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            ApiErrorCode::NotFound,
            "conversation not found",
        ));
    }
    let subject = page
        .items
        .last()
        .and_then(|message| message.subject.clone())
        .or_else(|| {
            page.items
                .iter()
                .find_map(|message| message.subject.clone())
        });
    Ok(Json(ConversationView {
        id: conversation_id,
        subject,
        messages: page.items,
    }))
}

/// GET /v1/sources/{source_id}/messages/{id}
///
/// Returns header + attachments only. The body is a separate lazy resource
/// (`GET .../body`), sanitized at that single chokepoint, so detail never
/// carries or serves the body.
///
/// @spec docs/L1-api#conversations-and-messages
/// @spec docs/L1-api#message-body-sanitization
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/messages/{message_id}",
    tag = "messages",
    summary = "Get message detail",
    description = "Returns full message detail with sanitized body HTML and rewritten inline \
                   attachment URLs.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("message_id" = String, Path, description = "Message identifier")
    ),
    responses(
        (status = 200, description = "The message detail", body = MessageDetail),
        (status = 404, description = "Message not found", body = ApiErrorBody)
    )
)]
pub async fn get_message(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
) -> Result<Json<MessageDetail>, ApiError> {
    let result = state
        .runtime
        .get_message_detail(
            RuntimeCaller::api(),
            AccountId(source_id.clone()),
            MessageId(message_id.clone()),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    let detail = result.detail.ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            ApiErrorCode::NotFound,
            "message detail not available",
        )
    })?;
    // Body-free by construction: the detail read (get_message_header) never loads
    // the body — it is the separate, sanitized `/body` lazy resource.
    Ok(Json(detail))
}

/// GET /v1/sources/{source_id}/messages/{message_id}/attachments/{attachment_id}
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/messages/{message_id}/attachments/{attachment_id}",
    tag = "messages",
    summary = "Get message attachment",
    description = "Returns the raw bytes of a message attachment, inline or as a download.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("message_id" = String, Path, description = "Message identifier"),
        ("attachment_id" = String, Path, description = "Attachment identifier"),
        GetAttachmentQuery
    ),
    responses(
        (status = 200, description = "Attachment bytes, served with the attachment's own MIME type (octet-stream fallback)", content_type = "*/*", body = [u8]),
        (status = 404, description = "Message or attachment not found", body = ApiErrorBody),
        (status = 502, description = "Upstream network error fetching the attachment", body = ApiErrorBody),
        (status = 503, description = "Account gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn get_message_attachment(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id, attachment_id)): Path<(String, String, String)>,
    Query(query): Query<GetAttachmentQuery>,
) -> Result<Response, ApiError> {
    serve_message_resource(
        &state,
        source_id,
        message_id,
        MessageResourceKind::Attachment(attachment_id),
        query.download.unwrap_or(false),
    )
    .await
}

/// GET /v1/sources/{source_id}/messages/{message_id}/body
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/messages/{message_id}/body",
    tag = "messages",
    summary = "Get message body",
    description = "Returns the message body as a lazy resource: sanitized HTML (default) or \
                   plain text, with inline attachment URLs rewritten. Served separately from \
                   message detail so a detail read never carries the body.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("message_id" = String, Path, description = "Message identifier"),
        GetBodyQuery
    ),
    responses(
        (status = 200, description = "Body bytes (text/html sanitized, or text/plain)", content_type = "*/*", body = [u8]),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 503, description = "Account gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn get_message_body(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
    Query(query): Query<GetBodyQuery>,
) -> Result<Response, ApiError> {
    let kind = match query.format.as_deref() {
        Some("text") => MessageResourceKind::BodyText,
        _ => MessageResourceKind::BodyHtml,
    };
    serve_message_resource(&state, source_id, message_id, kind, false).await
}
