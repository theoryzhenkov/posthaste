use super::*;

pub(crate) mod compose;
pub(crate) mod detail;
pub(crate) mod listing;
mod types;

pub use compose::{
    delete_draft, discard_operation, get_draft_content, get_identity, get_reply_context,
    list_pending_operations, retry_operation,
    list_sender_addresses, save_draft, send_message, DeleteDraftRequest, SaveDraftRequest,
};
pub use detail::{get_conversation, get_message, get_message_attachment};
pub use listing::{list_conversations, list_source_messages, search_messages};
pub use types::{
    ConversationPageResponse, GetAttachmentQuery, ListConversationsQuery,
    ListSmartMailboxMessagesQuery, ListSourceMessagesQuery, MessagePageResponse,
    SearchMessagesQuery,
};

#[cfg(test)]
pub(super) use compose::validate_send_message_request;
