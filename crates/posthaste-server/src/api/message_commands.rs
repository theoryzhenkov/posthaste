use super::*;

/// POST /v1/sources/{sid}/commands/messages/{mid}/set-keywords
///
/// @spec docs/L1-api#message-commands
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/commands/messages/{message_id}/set-keywords",
    tag = "messages",
    summary = "Set message keywords",
    description = "Adds and/or removes JMAP keywords on a message.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("message_id" = String, Path, description = "Message identifier")
    ),
    request_body = SetKeywordsCommand,
    responses(
        (status = 200, description = "Command result", body = CommandAck),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn set_keywords(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
    Json(command): Json<SetKeywordsCommand>,
) -> Result<Json<CommandAck>, ApiError> {
    state
        .runtime
        .set_message_keywords(
            RuntimeCaller::api(),
            AccountId(source_id),
            MessageId(message_id),
            command,
        )
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// POST /v1/sources/{sid}/commands/messages/{mid}/add-to-mailbox
///
/// @spec docs/L1-api#message-commands
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/commands/messages/{message_id}/add-to-mailbox",
    tag = "messages",
    summary = "Add message to mailbox",
    description = "Adds a message to a single additional mailbox.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("message_id" = String, Path, description = "Message identifier")
    ),
    request_body = AddToMailboxCommand,
    responses(
        (status = 200, description = "Command result", body = CommandAck),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn add_to_mailbox(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
    Json(command): Json<AddToMailboxCommand>,
) -> Result<Json<CommandAck>, ApiError> {
    state
        .runtime
        .add_message_to_mailbox(
            RuntimeCaller::api(),
            AccountId(source_id),
            MessageId(message_id),
            command,
        )
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// POST /v1/sources/{sid}/commands/messages/{mid}/remove-from-mailbox
///
/// @spec docs/L1-api#message-commands
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/commands/messages/{message_id}/remove-from-mailbox",
    tag = "messages",
    summary = "Remove message from mailbox",
    description = "Removes a message from a single mailbox.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("message_id" = String, Path, description = "Message identifier")
    ),
    request_body = RemoveFromMailboxCommand,
    responses(
        (status = 200, description = "Command result", body = CommandAck),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn remove_from_mailbox(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
    Json(command): Json<RemoveFromMailboxCommand>,
) -> Result<Json<CommandAck>, ApiError> {
    state
        .runtime
        .remove_message_from_mailbox(
            RuntimeCaller::api(),
            AccountId(source_id),
            MessageId(message_id),
            command,
        )
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// POST /v1/sources/{sid}/commands/messages/{mid}/replace-mailboxes
///
/// @spec docs/L1-api#message-commands
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/commands/messages/{message_id}/replace-mailboxes",
    tag = "messages",
    summary = "Replace message mailboxes",
    description = "Atomically replaces all mailbox memberships for a message.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("message_id" = String, Path, description = "Message identifier")
    ),
    request_body = ReplaceMailboxesCommand,
    responses(
        (status = 200, description = "Command result", body = CommandAck),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn replace_mailboxes(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
    Json(command): Json<ReplaceMailboxesCommand>,
) -> Result<Json<CommandAck>, ApiError> {
    state
        .runtime
        .replace_message_mailboxes(
            RuntimeCaller::api(),
            AccountId(source_id),
            MessageId(message_id),
            command,
        )
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// POST /v1/sources/{sid}/commands/messages/{mid}/destroy
///
/// @spec docs/L1-api#message-commands
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/commands/messages/{message_id}/destroy",
    tag = "messages",
    summary = "Destroy message",
    description = "Permanently deletes a message.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("message_id" = String, Path, description = "Message identifier")
    ),
    responses(
        (status = 200, description = "Command result", body = CommandAck),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn destroy_message(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
) -> Result<Json<CommandAck>, ApiError> {
    state
        .runtime
        .destroy_message(
            RuntimeCaller::api(),
            AccountId(source_id),
            MessageId(message_id),
        )
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}
