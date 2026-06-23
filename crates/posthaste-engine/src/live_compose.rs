mod attachments;
mod draft;
mod identity;
mod reply;
mod send;

pub(crate) use draft::{delete_draft, save_draft};
pub(crate) use identity::fetch_identity;
pub(crate) use reply::fetch_reply_context;
pub(crate) use send::send_message;

#[cfg(test)]
pub(crate) use identity::{resolve_draft_sender, resolve_send_identity};

#[cfg(test)]
mod tests;
