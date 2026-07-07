use axum::http::HeaderMap;

use crate::api::message_commands::idempotency_key;

use super::*;

/// Response body for `POST /v1/sources/{source_id}/commands/send`: the send
/// was accepted (enqueued local-first). `operation` is the enqueued outbox
/// send — its `id` is the CANCEL handle for a scheduled (`sendAt`) send
/// (`DELETE /v1/sources/{source_id}/operations/{operation_id}` before it is
/// due), and `sendAt` echoes the normalized schedule. `null` only for a keyed
/// replay whose outcome predates operation-bearing send records.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageResponse {
    pub ok: bool,
    pub operation: Option<Operation>,
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
    description = "Validates and enqueues a local-first send, then triggers a flush. With `sendAt` (RFC 3339) the send is HELD until due (undo-send / send-later) and can be canceled via the returned operation id while still queued. Local-first: a scheduled send fires when Posthaste is next running and online at/after `sendAt`, not via a server-side schedule.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("Idempotency-Key" = Option<String>, Header, description = "Client-supplied idempotency key (RFC-L2-scripting ruling 24): a redelivery under the same key returns the first outcome instead of enqueuing a second outbox send; reusing a key with a different operation is 409 Conflict.")
    ),
    request_body = SendMessageRequest,
    responses(
        (status = 200, description = "Message accepted for delivery (or held until `sendAt`)", body = SendMessageResponse),
        (status = 400, description = "Invalid compose request (including an invalid `sendAt`)", body = ApiErrorBody),
        (status = 409, description = "Idempotency key reused with a different operation", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn send_message(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, ApiError> {
    validate_send_message_request(&request)?;
    // Forward the `Idempotency-Key` (RFC-L2-scripting ruling 24) so an
    // at-least-once script's retried reply/send under the same key enqueues
    // exactly ONE outbox send. This ledger check guards the HTTP boundary (key →
    // one operation created); M32's outbox exactly-once (deterministic
    // `phsend-<op-id>`) then guards provider-side duplicates for that one
    // operation — they compose, key → one operation → one provider submission.
    let operation = state
        .runtime
        .send_message(
            RuntimeCaller::api(),
            AccountId(source_id),
            request,
            idempotency_key(&headers),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(SendMessageResponse {
        ok: true,
        operation,
    }))
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
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("Idempotency-Key" = Option<String>, Header, description = "Client-supplied idempotency key (RFC-L2-drafts D128): a redelivery under the same key returns the original operation (same id and response) instead of enqueuing a second draft; reusing a key with a different operation is 409 Conflict.")
    ),
    request_body = SaveDraftRequest,
    responses(
        (status = 200, description = "Draft operation enqueued", body = Operation),
        (status = 400, description = "Invalid draft request", body = ApiErrorBody),
        (status = 409, description = "Idempotency key reused with a different operation", body = ApiErrorBody),
        (status = 503, description = "Runtime unavailable", body = ApiErrorBody)
    )
)]
pub async fn save_draft(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    headers: HeaderMap,
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
            idempotency_key(&headers),
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
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("Idempotency-Key" = Option<String>, Header, description = "Client-supplied idempotency key (RFC-L2-drafts D128): a redelivery under the same key returns the original operation instead of enqueuing a second deletion; reusing a key with a different operation is 409 Conflict.")
    ),
    request_body = DeleteDraftRequest,
    responses(
        (status = 200, description = "Draft deletion enqueued", body = Operation),
        (status = 409, description = "Idempotency key reused with a different operation", body = ApiErrorBody),
        (status = 503, description = "Runtime unavailable", body = ApiErrorBody)
    )
)]
pub async fn delete_draft(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<DeleteDraftRequest>,
) -> Result<Json<Operation>, ApiError> {
    state
        .runtime
        .delete_draft(
            RuntimeCaller::api(),
            AccountId(source_id),
            idempotency_key(&headers),
            MessageId(request.draft_id),
        )
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
