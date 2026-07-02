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
