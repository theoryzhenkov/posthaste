use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Email {
    pub id: String,
    pub thread_id: String,
    pub blob_id: String,
    pub subject: Option<String>,
    pub from_name: Option<String>,
    pub from_email: Option<String>,
    pub preview: Option<String>,
    pub received_at: i64,
    pub has_attachment: bool,
    pub size: u64,
    pub mailbox_ids: Vec<String>,
    pub keywords: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mailbox {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub role: Option<String>,
    pub sort_order: u32,
    pub total_emails: u64,
    pub unread_emails: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Thread {
    pub id: String,
    pub email_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncState {
    pub mailbox_state: Option<String>,
    pub email_state: Option<String>,
    pub thread_state: Option<String>,
}
