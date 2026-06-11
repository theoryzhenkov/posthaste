use super::*;

pub(crate) mod crud;
pub(crate) mod listings;

pub use crud::{
    create_smart_mailbox, delete_smart_mailbox, get_smart_mailbox, list_smart_mailboxes,
    patch_smart_mailbox, reset_default_smart_mailboxes,
};
pub use listings::{list_smart_mailbox_conversations, list_smart_mailbox_messages};
