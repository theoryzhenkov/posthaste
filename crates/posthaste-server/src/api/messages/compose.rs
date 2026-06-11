use super::*;

/// GET /v1/sources/{source_id}/identity
///
/// @spec docs/L1-api#compose
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/identity",
    tag = "messages",
    summary = "Get sender identity",
    description = "Returns the JMAP sender identity for a source.",
    params(("source_id" = String, Path, description = "Source (account) identifier")),
    responses(
        (status = 200, description = "The sender identity", body = Identity),
        (status = 404, description = "Source not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn get_identity(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
) -> Result<Json<Identity>, ApiError> {
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    state
        .service
        .fetch_identity(&account_id, gateway.as_ref())
        .await
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// GET /v1/sender-addresses
///
/// @spec docs/L1-api#compose
#[utoipa::path(
    get,
    path = "/v1/sender-addresses",
    tag = "messages",
    summary = "List sender addresses",
    description = "Returns locally cached sender addresses that previously passed submission.",
    responses(
        (status = 200, description = "Cached sender addresses", body = [CachedSenderAddress]),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn list_sender_addresses(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<CachedSenderAddress>>, ApiError> {
    state
        .store
        .list_sender_address_cache()
        .map(Json)
        .map_err(store_error_to_api)
}

/// GET /v1/sources/{source_id}/messages/{id}/reply-context
///
/// @spec docs/L1-api#compose
/// @spec docs/L1-compose#reply-quoting
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/messages/{message_id}/reply-context",
    tag = "messages",
    summary = "Get reply context",
    description = "Returns pre-computed reply/forward metadata (recipients, subjects, quoted body).",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("message_id" = String, Path, description = "Message identifier")
    ),
    responses(
        (status = 200, description = "The reply context", body = ReplyContext),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn get_reply_context(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
) -> Result<Json<ReplyContext>, ApiError> {
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    state
        .service
        .fetch_reply_context(&account_id, &MessageId(message_id), gateway.as_ref())
        .await
        .map(Json)
        .map_err(ApiError::from_service_error)
}

/// POST /v1/sources/{source_id}/commands/send
///
/// @spec docs/L1-api#compose
/// @spec docs/L1-compose#no-send-empty-to
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/commands/send",
    tag = "messages",
    summary = "Send message",
    description = "Validates and submits a new email via the source gateway, then triggers a sync.",
    params(("source_id" = String, Path, description = "Source (account) identifier")),
    request_body = SendMessageRequest,
    responses(
        (status = 200, description = "Message accepted for delivery", body = OkResponse),
        (status = 400, description = "Invalid compose request", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn send_message(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    Json(request): Json<SendMessageRequest>,
) -> Result<Json<OkResponse>, ApiError> {
    validate_send_message_request(&request)?;
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    state
        .service
        .send_message(&account_id, &request, gateway.as_ref())
        .await
        .map_err(ApiError::from_service_error)?;
    if let Some(sender) = &request.from {
        if let Err(error) = state.store.remember_sender_address(&account_id, sender) {
            ph_warn!(
                events::SEND_SENDER_CACHE_UPDATE_FAILED,
                source_id = %account_id,
                sender = %sender.email,
                error = %error,
                "send accepted but sender address cache update failed"
            );
        }
    }
    if let Err(error) = state
        .supervisor
        .trigger_account_sync(&account_id, SyncTrigger::Manual)
        .await
    {
        ph_warn!(
            events::SEND_FOLLOWUP_SYNC_TRIGGER_FAILED,
            source_id = %account_id,
            error = %error,
            "send accepted but follow-up sync trigger failed"
        );
    }
    Ok(Json(OkResponse { ok: true }))
}

pub(crate) fn validate_send_message_request(request: &SendMessageRequest) -> Result<(), ApiError> {
    if request.from.as_ref().is_some_and(recipient_email_is_empty) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidCompose,
            "sender email address cannot be empty",
        ));
    }
    if request.to.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidCompose,
            "at least one To recipient is required",
        ));
    }
    if request.subject.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidCompose,
            "subject is required",
        ));
    }
    if request.body.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidCompose,
            "message body is required",
        ));
    }
    if request
        .to
        .iter()
        .chain(request.cc.iter())
        .chain(request.bcc.iter())
        .any(recipient_email_is_empty)
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidCompose,
            "recipient email addresses cannot be empty",
        ));
    }
    if request.attachments.len() > MAX_SEND_ATTACHMENTS {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidCompose,
            "too many attachments",
        ));
    }
    let mut total_attachment_bytes = 0_u64;
    for attachment in &request.attachments {
        if attachment.filename.trim().is_empty() {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidCompose,
                "attachment filename is required",
            ));
        }
        let attachment_size = decoded_attachment_size(attachment.content_base64.trim())
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    ApiErrorCode::InvalidCompose,
                    "attachment content must be valid base64",
                )
            })?;
        if attachment_size > MAX_SEND_ATTACHMENT_BYTES {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidCompose,
                "attachment is too large",
            ));
        }
        total_attachment_bytes = total_attachment_bytes.saturating_add(attachment_size);
        if total_attachment_bytes > MAX_SEND_TOTAL_ATTACHMENT_BYTES {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidCompose,
                "attachments are too large",
            ));
        }
    }
    Ok(())
}

fn recipient_email_is_empty(recipient: &Recipient) -> bool {
    recipient.email.trim().is_empty()
}

fn decoded_attachment_size(content: &str) -> Option<u64> {
    let mut decoder = base64::read::DecoderReader::new(
        content.as_bytes(),
        &base64::engine::general_purpose::STANDARD,
    );
    io::copy(&mut decoder, &mut io::sink()).ok()
}
