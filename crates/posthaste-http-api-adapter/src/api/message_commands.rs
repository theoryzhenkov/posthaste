use super::*;

use axum::http::HeaderMap;
use posthaste_contract_core::mutation_args::{
    MessageMailboxMembershipArgs, MessageReplaceMailboxesArgs, MessageSetKeywordsMutationArgs,
    MessageTargetArgs,
};
use posthaste_contract_core::{ClientMutationId, MailOperation};

/// The optional client-supplied idempotency key for a direct-apply command
/// (RFC-L2-scripting D53 / P8 fix). Carried as the `Idempotency-Key` request
/// header — the REST mirror of the replica path's `clientMutationId` body field
/// (a header is uniform across all five command routes, including the bodyless
/// `destroy`). A redelivery under the same key returns the first outcome instead
/// of re-executing; reusing a key with a different operation is a `409 Conflict`.
/// An empty/whitespace value is treated as absent.
///
/// @spec docs/eph/RFC-L2-scripting#6-d53-the-action-path
const IDEMPOTENCY_KEY_HEADER: &str = "idempotency-key";

fn idempotency_key(headers: &HeaderMap) -> Option<ClientMutationId> {
    headers
        .get(IDEMPOTENCY_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ClientMutationId::new)
}

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
        ("message_id" = String, Path, description = "Message identifier"),
        ("Idempotency-Key" = Option<String>, Header, description = "Client-supplied idempotency key (RFC-L2-scripting D53): a redelivery under the same key returns the first outcome instead of re-executing; reusing a key with a different operation is 409 Conflict.")
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
    headers: HeaderMap,
    Json(command): Json<SetKeywordsCommand>,
) -> Result<Json<CommandAck>, ApiError> {
    state
        .runtime
        .apply(
            RuntimeCaller::api(),
            MailOperation::SetKeywords(MessageSetKeywordsMutationArgs {
                source_id,
                message_id,
                command,
            }),
            idempotency_key(&headers),
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
        ("message_id" = String, Path, description = "Message identifier"),
        ("Idempotency-Key" = Option<String>, Header, description = "Client-supplied idempotency key (RFC-L2-scripting D53): a redelivery under the same key returns the first outcome instead of re-executing; reusing a key with a different operation is 409 Conflict.")
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
    headers: HeaderMap,
    Json(command): Json<AddToMailboxCommand>,
) -> Result<Json<CommandAck>, ApiError> {
    state
        .runtime
        .apply(
            RuntimeCaller::api(),
            MailOperation::AddToMailbox(MessageMailboxMembershipArgs {
                source_id,
                message_id,
                mailbox_id: command.mailbox_id.0,
            }),
            idempotency_key(&headers),
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
        ("message_id" = String, Path, description = "Message identifier"),
        ("Idempotency-Key" = Option<String>, Header, description = "Client-supplied idempotency key (RFC-L2-scripting D53): a redelivery under the same key returns the first outcome instead of re-executing; reusing a key with a different operation is 409 Conflict.")
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
    headers: HeaderMap,
    Json(command): Json<RemoveFromMailboxCommand>,
) -> Result<Json<CommandAck>, ApiError> {
    state
        .runtime
        .apply(
            RuntimeCaller::api(),
            MailOperation::RemoveFromMailbox(MessageMailboxMembershipArgs {
                source_id,
                message_id,
                mailbox_id: command.mailbox_id.0,
            }),
            idempotency_key(&headers),
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
        ("message_id" = String, Path, description = "Message identifier"),
        ("Idempotency-Key" = Option<String>, Header, description = "Client-supplied idempotency key (RFC-L2-scripting D53): a redelivery under the same key returns the first outcome instead of re-executing; reusing a key with a different operation is 409 Conflict.")
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
    headers: HeaderMap,
    Json(command): Json<ReplaceMailboxesCommand>,
) -> Result<Json<CommandAck>, ApiError> {
    state
        .runtime
        .apply(
            RuntimeCaller::api(),
            MailOperation::ReplaceMailboxes(MessageReplaceMailboxesArgs {
                source_id,
                message_id,
                mailbox_ids: command.mailbox_ids.into_iter().map(|id| id.0).collect(),
            }),
            idempotency_key(&headers),
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
        ("message_id" = String, Path, description = "Message identifier"),
        ("Idempotency-Key" = Option<String>, Header, description = "Client-supplied idempotency key (RFC-L2-scripting D53): a redelivery under the same key returns the first outcome instead of re-executing; reusing a key with a different operation is 409 Conflict.")
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
    headers: HeaderMap,
) -> Result<Json<CommandAck>, ApiError> {
    state
        .runtime
        .apply(
            RuntimeCaller::api(),
            MailOperation::Destroy(MessageTargetArgs {
                source_id,
                message_id,
            }),
            idempotency_key(&headers),
        )
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}
