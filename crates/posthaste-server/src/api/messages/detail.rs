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

/// Resolve a lazy message resource and serve it: fetch raw bytes from the
/// runtime, apply the per-kind transform policy, and build the byte response.
/// Every resource byte endpoint (attachment, body) goes through this one path.
pub(crate) async fn serve_message_resource(
    state: &Arc<AppState>,
    source_id: String,
    message_id: String,
    kind: MessageResourceKind,
    download: bool,
) -> Result<Response, ApiError> {
    let resource = state
        .runtime
        .get_message_resource(
            RuntimeCaller::api(),
            AccountId(source_id.clone()),
            MessageId(message_id.clone()),
            kind.clone(),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    let resource = apply_resource_transform(&source_id, &message_id, &kind, resource);
    serve_resource_response(resource, download)
}

/// The per-kind serve policy — the single place a resource's bytes are
/// transformed. Body HTML is sanitized then has its inline `cid:` URLs rewritten
/// (byte-identical to what the detail endpoint used to do); every other resource
/// is served verbatim.
fn apply_resource_transform(
    source_id: &str,
    message_id: &str,
    kind: &MessageResourceKind,
    resource: RuntimeResourceBytes,
) -> RuntimeResourceBytes {
    match kind {
        MessageResourceKind::BodyHtml => {
            let html = String::from_utf8_lossy(&resource.bytes);
            let sanitized = sanitize::sanitize_email_html(&html);
            let rewritten = rewrite_inline_attachment_urls(
                &sanitized,
                source_id,
                message_id,
                &resource.inline_attachments,
            );
            RuntimeResourceBytes {
                bytes: rewritten.into_bytes(),
                ..resource
            }
        }
        MessageResourceKind::Attachment(_) | MessageResourceKind::BodyText => resource,
    }
}

/// Build the HTTP response for a resolved lazy message resource: content type,
/// inline/attachment disposition, and the shared cache policy. Every resource
/// byte response (attachment, body) goes through this one builder.
pub(crate) fn serve_resource_response(
    resource: RuntimeResourceBytes,
    download: bool,
) -> Result<Response, ApiError> {
    let disposition_kind = if download { "attachment" } else { "inline" };
    let filename = resource.filename.as_deref().unwrap_or("resource");
    let content_disposition = format!(
        "{disposition_kind}; filename=\"{}\"",
        escape_content_disposition_filename(filename)
    );

    let mut response = Response::new(Body::from(resource.bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(resource.content_type.as_str())
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

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_domain::BlobId;

    fn inline_attachment(id: &str, cid: &str) -> MessageAttachment {
        MessageAttachment {
            id: id.to_string(),
            blob_id: BlobId::from("blob-1"),
            part_id: None,
            filename: None,
            mime_type: "image/png".to_string(),
            size: 0,
            disposition: Some("inline".to_string()),
            cid: Some(cid.to_string()),
            is_inline: true,
        }
    }

    // The body-html serve transform must reproduce the old detail behavior
    // exactly: sanitize first, then rewrite inline `cid:` URLs. This is the
    // security-critical path (XSS surface), so it is asserted directly.
    #[test]
    fn body_html_transform_sanitizes_then_rewrites_cid_urls() {
        let resource = RuntimeResourceBytes {
            bytes: br#"<script>alert(1)</script><img src="cid:img1"><p>hi</p>"#.to_vec(),
            content_type: "text/html; charset=utf-8".to_string(),
            filename: None,
            inline_attachments: vec![inline_attachment("att-1", "img1")],
        };
        let out = apply_resource_transform("acct", "msg", &MessageResourceKind::BodyHtml, resource);
        let html = String::from_utf8(out.bytes).expect("utf8");
        assert!(
            !html.contains("<script>"),
            "script must be sanitized out: {html}"
        );
        assert!(
            html.contains("/v1/sources/acct/messages/msg/attachments/att-1"),
            "cid must be rewritten to the attachment URL: {html}"
        );
        assert!(!html.contains("cid:img1"), "raw cid must be gone: {html}");
    }

    #[test]
    fn non_body_resources_are_served_verbatim() {
        let raw = b"\x00\x01raw-bytes<script>".to_vec();
        let resource = RuntimeResourceBytes {
            bytes: raw.clone(),
            content_type: "application/octet-stream".to_string(),
            filename: Some("f.bin".to_string()),
            inline_attachments: Vec::new(),
        };
        let out = apply_resource_transform(
            "a",
            "m",
            &MessageResourceKind::Attachment("x".to_string()),
            resource,
        );
        assert_eq!(out.bytes, raw);
    }
}
