use super::*;

/// Command to add and/or remove JMAP keywords on a message.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SetKeywordsCommand {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

/// Command to atomically replace all mailbox memberships for a message.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReplaceMailboxesCommand {
    pub mailbox_ids: Vec<MailboxId>,
}

/// Command to add a message to a single additional mailbox.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AddToMailboxCommand {
    pub mailbox_id: MailboxId,
}

/// Command to remove a message from a single mailbox.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RemoveFromMailboxCommand {
    pub mailbox_id: MailboxId,
}

/// Result of a message mutation: updated detail (if applicable) and emitted events.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CommandResult {
    pub detail: Option<MessageDetail>,
    pub events: Vec<DomainEvent>,
}

/// Server-side outcome of a gateway mutation, carrying an updated sync cursor.
///
/// @spec docs/L1-sync#conflict-model
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationOutcome {
    pub cursor: Option<SyncCursor>,
}

/// JMAP sender identity for an account.
///
/// @spec docs/L1-jmap#core-types
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub email: String,
}

/// Email address with optional display name.
///
/// @spec docs/L1-jmap#methods-used
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Recipient {
    pub name: Option<String>,
    pub email: String,
}

/// Locally cached sender address that previously passed provider submission.
///
/// @spec docs/L1-compose#sender-selection
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CachedSenderAddress {
    pub source_id: AccountId,
    pub name: Option<String>,
    pub email: String,
    pub last_used_at: String,
}

/// Pre-computed reply/forward metadata fetched from the gateway.
///
/// @spec docs/L1-jmap#methods-used
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReplyContext {
    pub to: Vec<Recipient>,
    pub cc: Vec<Recipient>,
    pub reply_subject: String,
    pub forward_subject: String,
    pub quoted_body: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

/// File attachment payload for an outgoing compose request.
///
/// The frontend sends base64 content to the daemon; the provider adapter uploads
/// or embeds the bytes using the transport-native attachment path before send.
///
/// @spec docs/L1-compose#attachment-handling
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SendMessageAttachment {
    pub filename: String,
    pub mime_type: String,
    pub content_base64: String,
}

/// Request payload for sending a new email via `EmailSubmission/set`.
///
/// @spec docs/L1-jmap#methods-used
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SendMessageRequest {
    pub from: Option<Recipient>,
    pub to: Vec<Recipient>,
    pub cc: Vec<Recipient>,
    pub bcc: Vec<Recipient>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    #[serde(default)]
    pub attachments: Vec<SendMessageAttachment>,
}
