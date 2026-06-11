use super::*;

/// Query parameters for conversation list endpoints.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListConversationsQuery {
    pub source_id: Option<String>,
    pub mailbox_id: Option<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub sort: Option<ConversationSortField>,
    pub sort_dir: Option<SortDirection>,
    pub q: Option<String>,
}

/// Query parameters for source-scoped message listing.
///
/// @spec docs/L1-api#conversations-and-messages
#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListSourceMessagesQuery {
    pub mailbox_id: Option<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub sort: Option<MessageSortField>,
    pub sort_dir: Option<SortDirection>,
    pub q: Option<String>,
}

/// Query parameters for global message search.
///
/// @spec docs/L1-api#conversations-and-messages
#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct SearchMessagesQuery {
    pub q: String,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub sort: Option<MessageSortField>,
    pub sort_dir: Option<SortDirection>,
}

/// Query parameters for smart-mailbox message listing.
///
/// @spec docs/L1-api#smart-mailboxes
#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ListSmartMailboxMessagesQuery {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub sort: Option<MessageSortField>,
    pub sort_dir: Option<SortDirection>,
    pub q: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct GetAttachmentQuery {
    pub download: Option<bool>,
}

/// Paginated conversation list response with an opaque cursor for the next page.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPageResponse {
    pub items: Vec<ConversationSummary>,
    pub next_cursor: Option<String>,
}

/// Paginated message list response with an opaque cursor for the next page.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MessagePageResponse {
    pub items: Vec<MessageSummary>,
    pub next_cursor: Option<String>,
}
