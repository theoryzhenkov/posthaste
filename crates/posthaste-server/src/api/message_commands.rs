use super::*;

/// POST /v1/sources/{sid}/commands/messages/{mid}/set-keywords
///
/// @spec docs/L1-api#message-commands
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
