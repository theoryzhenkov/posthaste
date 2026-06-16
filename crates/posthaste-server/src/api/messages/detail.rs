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
/// Sanitizes `body_html` through [`sanitize::sanitize_email_html`] before
/// returning to the frontend.
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
    let mut detail = result.detail.ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            ApiErrorCode::NotFound,
            "message detail not available",
        )
    })?;
    detail.body_html = detail
        .body_html
        .as_ref()
        .map(|html| sanitize::sanitize_email_html(html))
        .map(|html| {
            rewrite_inline_attachment_urls(&html, &source_id, &message_id, &detail.attachments)
        });
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
    let attachment = state
        .runtime
        .get_message_attachment(
            RuntimeCaller::api(),
            AccountId(source_id),
            MessageId(message_id),
            attachment_id,
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    let bytes = attachment.bytes;

    let disposition_kind = if query.download.unwrap_or(false) {
        "attachment"
    } else {
        "inline"
    };
    let filename = attachment.filename.as_deref().unwrap_or("attachment");
    let content_disposition = format!(
        "{disposition_kind}; filename=\"{}\"",
        escape_content_disposition_filename(filename)
    );

    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(attachment.mime_type.as_str())
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition)
            .map_err(|_| internal_error("invalid content disposition header".to_string()))?,
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=300"),
    );
    Ok(response)
}

fn rewrite_inline_attachment_urls(
    html: &str,
    source_id: &str,
    message_id: &str,
    attachments: &[MessageAttachment],
) -> String {
    let mut rewritten = html.to_string();
    for attachment in attachments {
        if !attachment.is_inline {
            continue;
        }
        let Some(cid) = attachment.cid.as_deref() else {
            continue;
        };
        let normalized = cid.trim().trim_start_matches('<').trim_end_matches('>');
        let url = format!(
            "/v1/sources/{source_id}/messages/{message_id}/attachments/{}",
            attachment.id
        );
        rewritten = rewritten.replace(&format!("cid:{normalized}"), &url);
        rewritten = rewritten.replace(&format!("cid:<{normalized}>"), &url);
    }
    rewritten
}

fn escape_content_disposition_filename(filename: &str) -> String {
    filename.replace('\\', "_").replace('"', "'")
}
