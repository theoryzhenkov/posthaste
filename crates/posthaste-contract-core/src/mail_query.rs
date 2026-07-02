use posthaste_domain_model::{
    ConversationCursor, ConversationPage, ConversationSortField, MessageCursor, MessagePage,
    MessageSortField, SortDirection,
};
use serde::{Deserialize, Serialize};

/// How a backend-evaluated message query should be presented in a page.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum MailPresentationRequest {
    Messages {
        limit: Option<usize>,
        cursor: Option<MessageCursor>,
        sort_field: MessageSortField,
        sort_direction: SortDirection,
    },
    CollapsedByConversation {
        limit: usize,
        cursor: Option<ConversationCursor>,
        sort_field: ConversationSortField,
        sort_direction: SortDirection,
    },
}

/// Best-effort cache-visibility side effect tied to a user-visible query page.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchVisibilityRequest {
    pub base_query: String,
    pub operation_id: Option<String>,
}

/// Transport-neutral mail query request.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailQueryRequest {
    pub query: String,
    pub presentation: MailPresentationRequest,
    pub visibility: Option<SearchVisibilityRequest>,
}

/// Page returned by a mail query presentation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "page")]
pub enum MailQueryPage {
    Messages(MessagePage),
    CollapsedByConversation(ConversationPage),
}
