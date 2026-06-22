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
    state
        .runtime
        .get_identity(RuntimeCaller::api(), AccountId(source_id))
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
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
        .runtime
        .list_sender_addresses(RuntimeCaller::api())
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
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
    state
        .runtime
        .get_reply_context(
            RuntimeCaller::api(),
            AccountId(source_id),
            MessageId(message_id),
        )
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// GET /v1/sources/{source_id}/messages/{id}/draft-content
///
/// @spec docs/L1-outbox#operation-model
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/messages/{message_id}/draft-content",
    tag = "messages",
    summary = "Get draft content",
    description = "Returns compose-ready content for resuming an existing draft, including Cc/Bcc when cached raw MIME is available.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("message_id" = String, Path, description = "Message identifier")
    ),
    responses(
        (status = 200, description = "The draft content", body = DraftContent),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn get_draft_content(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
) -> Result<Json<DraftContent>, ApiError> {
    state
        .runtime
        .get_draft_content(
            RuntimeCaller::api(),
            AccountId(source_id),
            MessageId(message_id),
        )
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
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
    state
        .runtime
        .send_message(RuntimeCaller::api(), AccountId(source_id), request)
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OkResponse { ok: true }))
}

/// Request body for `POST /v1/sources/{source_id}/commands/save-draft`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveDraftRequest {
    /// The existing draft id when editing; omit for a brand-new draft.
    #[serde(default)]
    pub draft_id: Option<String>,
    /// The draft content (same shape as a send request; all fields may be empty
    /// while composing).
    pub message: SendMessageRequest,
}

/// Request body for `POST /v1/sources/{source_id}/commands/delete-draft`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteDraftRequest {
    pub draft_id: String,
}

/// POST /v1/sources/{source_id}/commands/save-draft
///
/// @spec docs/L1-outbox#operation-model
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/commands/save-draft",
    tag = "messages",
    summary = "Save draft",
    description = "Enqueues a local-first draft create/update; flushed to the provider Drafts mailbox when connected.",
    params(("source_id" = String, Path, description = "Source (account) identifier")),
    request_body = SaveDraftRequest,
    responses(
        (status = 200, description = "Draft operation enqueued", body = Operation),
        (status = 400, description = "Invalid draft request", body = ApiErrorBody),
        (status = 503, description = "Runtime unavailable", body = ApiErrorBody)
    )
)]
pub async fn save_draft(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    Json(request): Json<SaveDraftRequest>,
) -> Result<Json<Operation>, ApiError> {
    if request
        .message
        .from
        .as_ref()
        .is_some_and(recipient_email_is_empty)
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidCompose,
            "sender email address cannot be empty",
        ));
    }
    state
        .runtime
        .save_draft(
            RuntimeCaller::api(),
            AccountId(source_id),
            request.draft_id.map(MessageId),
            request.message,
        )
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// POST /v1/sources/{source_id}/commands/delete-draft
///
/// @spec docs/L1-outbox#operation-model
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/commands/delete-draft",
    tag = "messages",
    summary = "Delete draft",
    description = "Enqueues a local-first draft deletion; flushed to the provider when connected.",
    params(("source_id" = String, Path, description = "Source (account) identifier")),
    request_body = DeleteDraftRequest,
    responses(
        (status = 200, description = "Draft deletion enqueued", body = Operation),
        (status = 503, description = "Runtime unavailable", body = ApiErrorBody)
    )
)]
pub async fn delete_draft(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    Json(request): Json<DeleteDraftRequest>,
) -> Result<Json<Operation>, ApiError> {
    state
        .runtime
        .delete_draft(
            RuntimeCaller::api(),
            AccountId(source_id),
            MessageId(request.draft_id),
        )
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// GET /v1/sources/{source_id}/operations
///
/// @spec docs/L1-outbox#operation-model
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/operations",
    tag = "messages",
    summary = "List pending operations",
    description = "Lists an account's non-terminal outbox operations (pending/failed work), oldest first.",
    params(("source_id" = String, Path, description = "Source (account) identifier")),
    responses(
        (status = 200, description = "Pending operations", body = [Operation]),
        (status = 503, description = "Runtime unavailable", body = ApiErrorBody)
    )
)]
pub async fn list_pending_operations(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
) -> Result<Json<Vec<Operation>>, ApiError> {
    state
        .runtime
        .list_pending_operations(RuntimeCaller::api(), AccountId(source_id))
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
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
