//! Compose-side message endpoints: identity/context reads, draft + send
//! commands, and pending-operation control. Split by concern to keep each file
//! focused; the handlers are re-exported here so route + OpenAPI paths address
//! them under `compose::`.

use super::*;

pub(crate) mod drafts;
pub(crate) mod operations;
pub(crate) mod reads;

pub use drafts::{
    delete_draft, save_draft, send_message, DeleteDraftRequest, SaveDraftRequest,
    SendMessageResponse,
};
pub use operations::{discard_operation, list_pending_operations, retry_operation};
pub use reads::{get_draft_content, get_identity, get_reply_context, list_sender_addresses};

#[cfg(test)]
pub(crate) use drafts::validate_send_message_request;
