uniffi::setup_scaffolding!();

use mail_engine::MailEngine;

#[derive(uniffi::Record)]
pub struct FfiMailbox {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub role: Option<String>,
    pub sort_order: u32,
    pub total_emails: u64,
    pub unread_emails: u64,
}

#[derive(uniffi::Record)]
pub struct FfiEmail {
    pub id: String,
    pub thread_id: String,
    pub subject: Option<String>,
    pub from_name: Option<String>,
    pub from_email: Option<String>,
    pub preview: Option<String>,
    pub received_at: i64,
    pub has_attachment: bool,
    pub keywords: Vec<String>,
    pub mailbox_ids: Vec<String>,
}

#[derive(uniffi::Object)]
pub struct MailClient {
    engine: MailEngine,
}

#[uniffi::export]
impl MailClient {
    #[uniffi::constructor]
    pub fn new() -> Self {
        MailClient {
            engine: MailEngine::new(),
        }
    }

    pub fn get_mailboxes(&self) -> Vec<FfiMailbox> {
        self.engine
            .get_mailboxes()
            .into_iter()
            .map(|m| FfiMailbox {
                id: m.id,
                name: m.name,
                parent_id: m.parent_id,
                role: m.role,
                sort_order: m.sort_order,
                total_emails: m.total_emails,
                unread_emails: m.unread_emails,
            })
            .collect()
    }

    pub fn get_emails(&self, mailbox_id: String) -> Vec<FfiEmail> {
        self.engine
            .get_emails(&mailbox_id)
            .into_iter()
            .map(email_to_ffi)
            .collect()
    }

    pub fn get_all_emails(&self) -> Vec<FfiEmail> {
        self.engine
            .get_all_emails()
            .into_iter()
            .map(email_to_ffi)
            .collect()
    }
}

fn email_to_ffi(e: jmap_core::Email) -> FfiEmail {
    FfiEmail {
        id: e.id,
        thread_id: e.thread_id,
        subject: e.subject,
        from_name: e.from_name,
        from_email: e.from_email,
        preview: e.preview,
        received_at: e.received_at,
        has_attachment: e.has_attachment,
        keywords: e.keywords,
        mailbox_ids: e.mailbox_ids,
    }
}
