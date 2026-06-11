use super::*;

/// Account-qualified reference to a specific message.
///
/// @spec docs/L0-accounts#the-invariant
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SourceMessageRef {
    pub source_id: AccountId,
    pub message_id: MessageId,
}

/// Conversation row for the paginated middle pane.
///
/// @spec docs/L1-sync#conversation-pagination
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConversationSummary {
    pub id: ConversationId,
    pub subject: Option<String>,
    pub preview: Option<String>,
    pub from_name: Option<String>,
    pub from_email: Option<String>,
    pub latest_received_at: String,
    pub unread_count: i64,
    pub message_count: i64,
    pub source_ids: Vec<AccountId>,
    pub source_names: Vec<String>,
    pub latest_message: SourceMessageRef,
    pub latest_source_name: String,
    pub has_attachment: bool,
    pub is_flagged: bool,
}

/// Column by which conversation lists can be sorted.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ConversationSortField {
    #[default]
    Date,
    From,
    Subject,
    Source,
    ThreadSize,
    Flagged,
    Attachment,
}

/// Sort direction for conversation lists.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SortDirection {
    Asc,
    #[default]
    Desc,
}

/// Opaque seek-pagination cursor for conversation lists.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationCursor {
    pub sort_value: String,
    pub conversation_id: ConversationId,
}

/// A single page of conversation summaries with an optional cursor for the next page.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPage {
    pub items: Vec<ConversationSummary>,
    pub next_cursor: Option<ConversationCursor>,
}

/// Full conversation detail with all messages expanded.
///
/// @spec docs/L1-api#conversations-and-messages
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConversationView {
    pub id: ConversationId,
    pub subject: Option<String>,
    pub messages: Vec<MessageSummary>,
}
