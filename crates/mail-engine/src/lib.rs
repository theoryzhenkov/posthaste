pub mod mock;

use jmap_core::{Email, Mailbox, Thread};

/// The core engine. For the vertical slice, it just serves mock data.
/// Later this will connect to a real JMAP server.
pub struct MailEngine {}

impl MailEngine {
    pub fn new() -> Self {
        MailEngine {}
    }

    /// Get all mailboxes for the account.
    pub fn get_mailboxes(&self) -> Vec<Mailbox> {
        mock::generate_mailboxes()
    }

    /// Get emails in a specific mailbox.
    pub fn get_emails(&self, mailbox_id: &str) -> Vec<Email> {
        mock::generate_emails()
            .into_iter()
            .filter(|e| e.mailbox_ids.contains(&mailbox_id.to_string()))
            .collect()
    }

    /// Get all emails across all mailboxes.
    pub fn get_all_emails(&self) -> Vec<Email> {
        mock::generate_emails()
    }

    /// Get a thread by ID.
    pub fn get_thread(&self, thread_id: &str) -> Option<Thread> {
        mock::generate_threads()
            .into_iter()
            .find(|t| t.id == thread_id)
    }
}
