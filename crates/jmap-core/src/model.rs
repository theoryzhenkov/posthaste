use std::fmt::{Display, Formatter};
use std::pin::Pin;

use futures_util::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
    };
}

string_id!(AccountId);
string_id!(MailboxId);
string_id!(MessageId);
string_id!(ThreadId);
string_id!(BlobId);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub default_account_id: Option<AccountId>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            default_account_id: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountDriver {
    Jmap,
    Mock,
}

impl AccountDriver {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Jmap => "jmap",
            Self::Mock => "mock",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretKind {
    Env,
    Os,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretRef {
    pub kind: SecretKind,
    pub key: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretStorage {
    Env,
    Os,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretStatus {
    pub storage: SecretStorage,
    pub configured: bool,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountTransportSettings {
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub secret_ref: Option<SecretRef>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettings {
    pub id: AccountId,
    pub name: String,
    pub driver: AccountDriver,
    pub enabled: bool,
    pub transport: AccountTransportSettings,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountTransportOverview {
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub secret: SecretStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountStatus {
    Ready,
    Syncing,
    Degraded,
    AuthError,
    Offline,
    Disabled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PushStatus {
    Connected,
    Reconnecting,
    Unsupported,
    Disabled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRuntimeOverview {
    pub status: AccountStatus,
    pub push: PushStatus,
    pub last_sync_at: Option<String>,
    pub last_sync_error: Option<String>,
    pub last_sync_error_code: Option<String>,
}

impl Default for AccountRuntimeOverview {
    fn default() -> Self {
        Self {
            status: AccountStatus::Offline,
            push: PushStatus::Disabled,
            last_sync_at: None,
            last_sync_error: None,
            last_sync_error_code: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountOverview {
    pub id: AccountId,
    pub name: String,
    pub driver: AccountDriver,
    pub enabled: bool,
    pub transport: AccountTransportOverview,
    pub created_at: String,
    pub updated_at: String,
    pub is_default: bool,
    #[serde(flatten)]
    pub runtime: AccountRuntimeOverview,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncCursor {
    pub object_type: SyncObject,
    pub state: String,
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncObject {
    Mailbox,
    Message,
}

impl SyncObject {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mailbox => "mailbox",
            Self::Message => "message",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawMessageRef {
    pub path: String,
    pub sha256: String,
    pub size: i64,
    pub mime_type: String,
    pub fetched_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxSummary {
    pub id: MailboxId,
    pub name: String,
    pub role: Option<String>,
    pub unread_emails: i64,
    pub total_emails: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSummary {
    pub id: MessageId,
    pub thread_id: ThreadId,
    pub subject: Option<String>,
    pub from_name: Option<String>,
    pub from_email: Option<String>,
    pub preview: Option<String>,
    pub received_at: String,
    pub has_attachment: bool,
    pub is_read: bool,
    pub is_flagged: bool,
    pub mailbox_ids: Vec<MailboxId>,
    pub keywords: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetail {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub raw_message: Option<RawMessageRef>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadView {
    pub id: ThreadId,
    pub messages: Vec<MessageSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxRecord {
    pub id: MailboxId,
    pub name: String,
    pub role: Option<String>,
    pub unread_emails: i64,
    pub total_emails: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRecord {
    pub id: MessageId,
    pub thread_id: ThreadId,
    pub remote_blob_id: Option<BlobId>,
    pub subject: Option<String>,
    pub from_name: Option<String>,
    pub from_email: Option<String>,
    pub preview: Option<String>,
    pub received_at: String,
    pub has_attachment: bool,
    pub size: i64,
    pub mailbox_ids: Vec<MailboxId>,
    pub keywords: Vec<String>,
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub raw_mime: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncBatch {
    pub mailboxes: Vec<MailboxRecord>,
    pub messages: Vec<MessageRecord>,
    pub deleted_mailbox_ids: Vec<MailboxId>,
    pub deleted_message_ids: Vec<MessageId>,
    pub cursors: Vec<SyncCursor>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchedBody {
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub raw_mime: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainEvent {
    pub seq: i64,
    pub account_id: AccountId,
    pub topic: String,
    pub occurred_at: String,
    pub mailbox_id: Option<MailboxId>,
    pub message_id: Option<MessageId>,
    pub payload: Value,
}

#[derive(Clone, Debug)]
pub struct EventFilter {
    pub account_id: Option<AccountId>,
    pub topic: Option<String>,
    pub mailbox_id: Option<MailboxId>,
    pub after_seq: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncTrigger {
    Startup,
    Poll,
    Push,
    Manual,
}

impl SyncTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Poll => "poll",
            Self::Push => "push",
            Self::Manual => "manual",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushNotification {
    pub account_id: AccountId,
    pub changed: Vec<String>,
    pub received_at: String,
    pub checkpoint: Option<String>,
}

pub type PushStream = Pin<Box<dyn Stream<Item = Result<PushNotification, GatewayError>> + Send>>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetKeywordsCommand {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceMailboxesCommand {
    pub mailbox_ids: Vec<MailboxId>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddToMailboxCommand {
    pub mailbox_id: MailboxId,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveFromMailboxCommand {
    pub mailbox_id: MailboxId,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    pub detail: Option<MessageDetail>,
    pub events: Vec<DomainEvent>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub email: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipient {
    pub name: Option<String>,
    pub email: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplyContext {
    pub to: Vec<Recipient>,
    pub cc: Vec<Recipient>,
    pub reply_subject: String,
    pub forward_subject: String,
    pub quoted_body: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    pub to: Vec<Recipient>,
    pub cc: Vec<Recipient>,
    pub bcc: Vec<Recipient>,
    pub subject: String,
    pub body: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("gateway unavailable for account {0}")]
    Unavailable(String),
    #[error("authentication failed")]
    Auth,
    #[error("network error: {0}")]
    Network(String),
    #[error("state mismatch")]
    StateMismatch,
    #[error("cannot calculate changes")]
    CannotCalculateChanges,
    #[error("gateway rejected the request: {0}")]
    Rejected(String),
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("storage failure: {0}")]
    Failure(String),
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Gateway(#[from] GatewayError),
    #[error(transparent)]
    Secret(#[from] SecretStoreError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl ServiceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Gateway(GatewayError::Unavailable(_)) => "gateway_unavailable",
            Self::Gateway(GatewayError::Auth) => "auth_error",
            Self::Gateway(GatewayError::Network(_)) => "network_error",
            Self::Gateway(GatewayError::StateMismatch) => "state_mismatch",
            Self::Gateway(GatewayError::CannotCalculateChanges) => "cannot_calculate_changes",
            Self::Gateway(GatewayError::Rejected(_)) => "gateway_rejected",
            Self::Secret(SecretStoreError::Unavailable(_)) => "secret_unavailable",
            Self::Secret(SecretStoreError::Unsupported(_)) => "secret_unsupported",
            Self::Store(StoreError::NotFound(_)) => "not_found",
            Self::Store(StoreError::Conflict(_)) => "conflict",
            Self::Store(StoreError::Failure(_)) => "storage_failure",
        }
    }
}

#[derive(Debug, Error)]
pub enum SecretStoreError {
    #[error("secret unavailable: {0}")]
    Unavailable(String),
    #[error("secret store does not support operation: {0}")]
    Unsupported(String),
}
