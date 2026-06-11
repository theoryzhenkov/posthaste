use super::*;

pub(crate) mod compose;
pub(crate) mod detail;
pub(crate) mod listing;
mod support;
mod types;

pub use compose::{get_identity, get_reply_context, list_sender_addresses, send_message};
pub use detail::{get_conversation, get_message, get_message_attachment};
pub use listing::{list_conversations, list_source_messages, search_messages};
pub use types::{
    ConversationPageResponse, GetAttachmentQuery, ListConversationsQuery,
    ListSmartMailboxMessagesQuery, ListSourceMessagesQuery, MessagePageResponse,
    SearchMessagesQuery,
};

pub(crate) use support::live_gateway;
use support::{optional_live_gateway, require_live_gateway};

#[cfg(test)]
pub(super) use compose::validate_send_message_request;
