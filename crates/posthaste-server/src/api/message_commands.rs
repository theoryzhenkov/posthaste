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
        (status = 200, description = "Command result", body = CommandResult),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn set_keywords(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
    Json(command): Json<SetKeywordsCommand>,
) -> Result<Json<CommandResult>, ApiError> {
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    command_result_response(
        state.as_ref(),
        state
            .service
            .set_keywords(
                &account_id,
                &MessageId(message_id),
                &command,
                gateway.as_ref(),
            )
            .await,
    )
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
        (status = 200, description = "Command result", body = CommandResult),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn add_to_mailbox(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
    Json(command): Json<AddToMailboxCommand>,
) -> Result<Json<CommandResult>, ApiError> {
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    command_result_response(
        state.as_ref(),
        state
            .service
            .add_to_mailbox(
                &account_id,
                &MessageId(message_id),
                &command,
                gateway.as_ref(),
            )
            .await,
    )
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
        (status = 200, description = "Command result", body = CommandResult),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn remove_from_mailbox(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
    Json(command): Json<RemoveFromMailboxCommand>,
) -> Result<Json<CommandResult>, ApiError> {
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    command_result_response(
        state.as_ref(),
        state
            .service
            .remove_from_mailbox(
                &account_id,
                &MessageId(message_id),
                &command,
                gateway.as_ref(),
            )
            .await,
    )
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
        (status = 200, description = "Command result", body = CommandResult),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn replace_mailboxes(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
    Json(command): Json<ReplaceMailboxesCommand>,
) -> Result<Json<CommandResult>, ApiError> {
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    command_result_response(
        state.as_ref(),
        state
            .service
            .replace_mailboxes(
                &account_id,
                &MessageId(message_id),
                &command,
                gateway.as_ref(),
            )
            .await,
    )
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
        (status = 200, description = "Command result", body = CommandResult),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn destroy_message(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
) -> Result<Json<CommandResult>, ApiError> {
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    command_result_response(
        state.as_ref(),
        state
            .service
            .destroy_message(&account_id, &MessageId(message_id), gateway.as_ref())
            .await,
    )
}
