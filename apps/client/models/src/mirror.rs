//! TypeScript-shape twins of the domain-model types the wire reuses.
//!
//! The domain crate does not derive `ts_rs::TS`, so each reused type gets a
//! twin here that declares the identical serde shape and derives `TS`. Wire
//! structs keep their fields typed with the REAL domain types (the backend
//! serializes domain values untouched) and point ts-rs at these twins with
//! `#[ts(as = ...)]`.
//!
//! Drift protection: every struct here is `deny_unknown_fields`, and
//! `tests/mirror_drift.rs` serializes fully-populated domain values and
//! strictly decodes them into these twins — a renamed, removed, or added
//! domain field fails the test (or the exhaustive literals in it stop
//! compiling). These types are for TS generation and drift checks only; the
//! backend never constructs them.

use serde::Deserialize;
use ts_rs::TS;

/// Declares a TypeScript alias twin (`type Name = string`) for a domain
/// string-id newtype.
macro_rules! mirror_string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Deserialize, TS)]
        pub struct $name(pub String);
    };
}

mirror_string_id!(
    /// Twin of [`posthaste_domain_model::AccountId`].
    AccountId
);
mirror_string_id!(
    /// Twin of [`posthaste_domain_model::MailboxId`].
    MailboxId
);
mirror_string_id!(
    /// Twin of [`posthaste_domain_model::MessageId`].
    MessageId
);
mirror_string_id!(
    /// Twin of [`posthaste_domain_model::ThreadId`].
    ThreadId
);
mirror_string_id!(
    /// Twin of [`posthaste_domain_model::ConversationId`].
    ConversationId
);
mirror_string_id!(
    /// Twin of [`posthaste_domain_model::BlobId`].
    BlobId
);
mirror_string_id!(
    /// Twin of [`posthaste_domain_model::OperationId`].
    OperationId
);

/// Twin of [`posthaste_domain_model::Recipient`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Recipient {
    pub name: Option<String>,
    pub email: String,
}

/// Twin of [`posthaste_domain_model::MessageSummary`] — the list-row
/// projection.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageSummary {
    pub id: MessageId,
    pub source_id: AccountId,
    pub source_name: String,
    pub source_thread_id: ThreadId,
    pub conversation_id: ConversationId,
    pub subject: Option<String>,
    pub from_name: Option<String>,
    pub from_email: Option<String>,
    pub to: Vec<Recipient>,
    pub preview: Option<String>,
    pub received_at: String,
    pub has_attachment: bool,
    pub is_read: bool,
    pub is_flagged: bool,
    pub mailbox_ids: Vec<MailboxId>,
    pub keywords: Vec<String>,
    #[serde(default)]
    #[ts(optional, type = "number")]
    pub version: Option<u64>,
    #[serde(default)]
    #[ts(optional)]
    pub rfc_message_id: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub in_reply_to: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub draft_id: Option<String>,
}

/// Twin of [`posthaste_domain_model::MailboxSummary`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MailboxSummary {
    pub id: MailboxId,
    pub name: String,
    pub role: Option<String>,
    #[ts(type = "number")]
    pub unread_emails: i64,
    #[ts(type = "number")]
    pub total_emails: i64,
}

/// Twin of [`posthaste_domain_model::MessageAttachment`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageAttachment {
    pub id: String,
    pub blob_id: BlobId,
    pub part_id: Option<String>,
    pub filename: Option<String>,
    pub mime_type: String,
    #[ts(type = "number")]
    pub size: i64,
    pub disposition: Option<String>,
    pub cid: Option<String>,
    pub is_inline: bool,
}

/// Twin of [`posthaste_domain_model::ListUnsubscribe`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListUnsubscribe {
    #[serde(default)]
    #[ts(optional)]
    pub https: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub mailto: Option<String>,
    #[serde(default)]
    pub one_click: bool,
}

/// Twin of [`posthaste_domain_model::ThreadView`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadView {
    pub id: ThreadId,
    pub messages: Vec<MessageSummary>,
}

/// Twin of [`posthaste_domain_model::MessageSortField`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum MessageSortField {
    Date,
    From,
    Subject,
    Source,
    Flagged,
    Attachment,
}

/// Twin of [`posthaste_domain_model::SetKeywordsCommand`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetKeywordsCommand {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

/// Twin of [`posthaste_domain_model::ReplaceMailboxesCommand`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceMailboxesCommand {
    pub mailbox_ids: Vec<MailboxId>,
}

/// Twin of [`posthaste_domain_model::SendMessageAttachment`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendMessageAttachment {
    pub filename: String,
    pub mime_type: String,
    pub content_base64: String,
}

/// Twin of [`posthaste_domain_model::SendMessageRequest`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendMessageRequest {
    pub from: Option<Recipient>,
    pub to: Vec<Recipient>,
    pub cc: Vec<Recipient>,
    pub bcc: Vec<Recipient>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    #[serde(default)]
    pub attachments: Vec<SendMessageAttachment>,
    #[serde(default)]
    pub draft_id: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub send_at: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub undo_window_seconds: Option<u32>,
}

/// Twin of [`posthaste_domain_model::OperationKind`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    SetKeywords,
    ReplaceMailboxes,
    Destroy,
    DraftCreate,
    DraftUpdate,
    DraftDelete,
    Send,
}

/// Twin of [`posthaste_domain_model::OperationState`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum OperationState {
    Pending,
    Inflight,
    Applied,
    Failed,
    DispatchUncertain,
}

/// Twin of [`posthaste_domain_model::OperationEntityKind`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum OperationEntityKind {
    Message,
    Draft,
}

/// Twin of [`posthaste_domain_model::AccountStatus`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AccountStatus {
    Ready,
    Syncing,
    Degraded,
    AuthError,
    Offline,
    Disabled,
}

/// Twin of [`posthaste_domain_model::PushStatus`].
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PushStatus {
    Connected,
    Reconnecting,
    Unsupported,
    Disabled,
}
