use std::fmt::{Display, Formatter};
use std::pin::Pin;

use futures_util::Stream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::ConfigError;

/// Generates a newtype wrapper around `String` for type-safe identifiers.
macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
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

string_id!(
    /// Opaque server-assigned identifier for a mail account.
    ///
    /// @spec docs/L0-accounts#the-invariant
    AccountId
);

string_id!(
    /// Opaque server-assigned identifier for a mailbox (folder or label).
    ///
    /// @spec docs/L1-jmap#core-types
    MailboxId
);

string_id!(
    /// Opaque server-assigned identifier for a single email message.
    ///
    /// @spec docs/L1-jmap#core-types
    MessageId
);

string_id!(
    /// Opaque server-assigned identifier for a JMAP thread.
    ///
    /// @spec docs/L1-jmap#core-types
    ThreadId
);

string_id!(
    /// Opaque server-assigned identifier for a binary blob (attachment or body content).
    ///
    /// @spec docs/L1-jmap#methods-used
    BlobId
);

string_id!(
    /// Locally-derived identifier for a conversation (cross-source thread grouping).
    ///
    /// @spec docs/L1-sync#conversation-pagination
    ConversationId
);

string_id!(
    /// Identifier for a smart mailbox (saved query with display metadata).
    ///
    /// @spec docs/L1-search#smart-mailbox-data-model
    SmartMailboxId
);

/// Default timestamp for missing `created_at`/`updated_at` fields in config.
///
/// @spec docs/L1-accounts#toml-schema
pub const RFC3339_EPOCH: &str = "1970-01-01T00:00:00Z";

/// Event topic emitted after a successful sync cycle completes.
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_SYNC_COMPLETED: &str = "sync.completed";

/// Event topic emitted when a sync cycle fails.
///
/// @spec docs/L1-sync#error-handling
pub const EVENT_TOPIC_SYNC_FAILED: &str = "sync.failed";

/// Event topic emitted when message metadata changes (keywords, mailboxes).
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_MESSAGE_UPDATED: &str = "message.updated";

/// Event topic emitted when a new message arrives in a mailbox.
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_MESSAGE_ARRIVED: &str = "message.arrived";

/// Event topic emitted when a mailbox is created, updated, or deleted.
///
/// @spec docs/L1-sync#event-propagation
pub const EVENT_TOPIC_MAILBOX_UPDATED: &str = "mailbox.updated";

/// Event topic emitted when account configuration changes.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub const EVENT_TOPIC_ACCOUNT_UPDATED: &str = "account.updated";

/// Event topic emitted when a new account is created.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub const EVENT_TOPIC_ACCOUNT_CREATED: &str = "account.created";

/// Event topic emitted when an account is deleted.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub const EVENT_TOPIC_ACCOUNT_DELETED: &str = "account.deleted";

/// Event topic emitted when account runtime status transitions.
///
/// @spec docs/L1-api#account-crud-lifecycle
pub const EVENT_TOPIC_ACCOUNT_STATUS_CHANGED: &str = "account.status_changed";

/// Event topic emitted when a push transport connects successfully.
///
/// @spec docs/L2-transport#push-transport
pub const EVENT_TOPIC_PUSH_CONNECTED: &str = "push.connected";

/// Event topic emitted when a push transport disconnects or fails.
///
/// @spec docs/L2-transport#push-transport
pub const EVENT_TOPIC_PUSH_DISCONNECTED: &str = "push.disconnected";

/// Global application settings shared across all accounts.
///
/// @spec docs/L1-accounts#toml-schema
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub default_account_id: Option<AccountId>,
}

/// Backend driver type for an account.
///
/// @spec docs/L1-accounts#toml-schema
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

/// Storage backend for account credentials.
///
/// @spec docs/L1-api#secret-management
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretKind {
    /// Credential read from an environment variable.
    Env,
    /// Credential stored in the OS keyring (macOS Keychain).
    Os,
}

/// Pointer to a stored secret, combining storage kind and lookup key.
///
/// @spec docs/L1-api#secret-management
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretRef {
    pub kind: SecretKind,
    pub key: String,
}

/// Alias for [`SecretKind`], used in API responses to describe where a secret is stored.
///
/// @spec docs/L1-api#secret-management
pub type SecretStorage = SecretKind;

/// Redacted secret status returned in API responses. Never contains the secret value.
///
/// @spec docs/L1-api#secret-management
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretStatus {
    pub storage: SecretStorage,
    pub configured: bool,
    pub label: Option<String>,
}

/// Transport-layer settings for connecting to a JMAP server.
///
/// @spec docs/L1-accounts#toml-schema
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountTransportSettings {
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub secret_ref: Option<SecretRef>,
}

/// User-facing visual identity for an account.
///
/// @spec docs/L1-accounts#toml-schema
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum AccountAppearance {
    Initials {
        initials: String,
        color_hue: u16,
    },
    Image {
        image_id: String,
        initials: String,
        color_hue: u16,
    },
}

/// Full persisted configuration for a mail account.
///
/// @spec docs/L1-accounts#toml-schema
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSettings {
    pub id: AccountId,
    pub name: String,
    pub full_name: Option<String>,
    pub email_patterns: Vec<String>,
    pub driver: AccountDriver,
    pub enabled: bool,
    pub appearance: Option<AccountAppearance>,
    pub transport: AccountTransportSettings,
    pub created_at: String,
    pub updated_at: String,
}

/// API-facing transport summary with redacted secret status.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountTransportOverview {
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub secret: SecretStatus,
}

/// Runtime health status of a mail account.
///
/// @spec docs/L1-api#account-crud-lifecycle
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

/// Current state of the push notification transport for an account.
///
/// @spec docs/L2-transport#push-transport
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PushStatus {
    Connected,
    Reconnecting,
    Unsupported,
    Disabled,
}

/// Volatile runtime state for an account (sync status, push status, last error).
///
/// @spec docs/L1-api#account-crud-lifecycle
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

/// Combined account config and runtime state returned by the API.
///
/// @spec docs/L1-api#account-crud-lifecycle
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountOverview {
    pub id: AccountId,
    pub name: String,
    pub full_name: Option<String>,
    pub email_patterns: Vec<String>,
    pub driver: AccountDriver,
    pub enabled: bool,
    pub appearance: AccountAppearance,
    pub transport: AccountTransportOverview,
    pub created_at: String,
    pub updated_at: String,
    pub is_default: bool,
    #[serde(flatten)]
    pub runtime: AccountRuntimeOverview,
}

/// Per-type, per-account JMAP state string used for delta sync.
///
/// @spec docs/L1-sync#state-management
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncCursor {
    pub object_type: SyncObject,
    pub state: String,
    pub updated_at: String,
}

/// JMAP object type that participates in delta sync.
///
/// @spec docs/L1-sync#state-management
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

/// Metadata for a locally-cached raw MIME message file.
///
/// @spec docs/L1-sync#sync-loop
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawMessageRef {
    pub path: String,
    pub sha256: String,
    pub size: i64,
    pub mime_type: String,
    pub fetched_at: String,
}

/// Lightweight mailbox view for sidebar and list endpoints.
///
/// @spec docs/L1-api#navigation
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxSummary {
    pub id: MailboxId,
    pub name: String,
    pub role: Option<String>,
    pub unread_emails: i64,
    pub total_emails: i64,
}

/// Message metadata for list views (no body content).
///
/// @spec docs/L1-api#conversations-and-messages
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSummary {
    pub id: MessageId,
    pub source_id: AccountId,
    pub source_name: String,
    pub source_thread_id: ThreadId,
    pub conversation_id: ConversationId,
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

/// Column by which message lists can be sorted.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageSortField {
    #[default]
    Date,
    From,
    Subject,
    Source,
    Flagged,
    Attachment,
}

/// Opaque seek-pagination cursor for message lists.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageCursor {
    pub sort_value: String,
    pub source_id: AccountId,
    pub message_id: MessageId,
}

/// A single page of message summaries with an optional cursor for the next page.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub items: Vec<MessageSummary>,
    pub next_cursor: Option<MessageCursor>,
}

/// Full message including sanitized body content, returned by message detail endpoint.
///
/// @spec docs/L1-api#message-body-sanitization
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageAttachment {
    pub id: String,
    pub blob_id: BlobId,
    pub part_id: Option<String>,
    pub filename: Option<String>,
    pub mime_type: String,
    pub size: i64,
    pub disposition: Option<String>,
    pub cid: Option<String>,
    pub is_inline: bool,
}

/// Full message including sanitized body content, returned by message detail endpoint.
///
/// @spec docs/L1-api#message-body-sanitization
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetail {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub raw_message: Option<RawMessageRef>,
    pub attachments: Vec<MessageAttachment>,
}

/// All messages belonging to a single JMAP thread, ordered by `receivedAt`.
///
/// @spec docs/L1-search#thread-view
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadView {
    pub id: ThreadId,
    pub messages: Vec<MessageSummary>,
}

/// Account-qualified reference to a specific message.
///
/// @spec docs/L0-accounts#the-invariant
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceMessageRef {
    pub source_id: AccountId,
    pub message_id: MessageId,
}

/// Conversation row for the paginated middle pane.
///
/// @spec docs/L1-sync#conversation-pagination
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummary {
    pub id: ConversationId,
    pub subject: Option<String>,
    pub preview: Option<String>,
    pub from_name: Option<String>,
    pub from_email: Option<String>,
    pub latest_received_at: String,
    pub unread_count: i64,
    pub message_count: i64,
    pub source_ids: Vec<AccountId>,
    pub source_names: Vec<String>,
    pub latest_message: SourceMessageRef,
    pub latest_source_name: String,
    pub has_attachment: bool,
    pub is_flagged: bool,
}

/// Column by which conversation lists can be sorted.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConversationSortField {
    #[default]
    Date,
    From,
    Subject,
    Source,
    ThreadSize,
    Flagged,
    Attachment,
}

/// Sort direction for conversation lists.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SortDirection {
    Asc,
    #[default]
    Desc,
}

/// Opaque seek-pagination cursor for conversation lists.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationCursor {
    pub sort_value: String,
    pub conversation_id: ConversationId,
}

/// A single page of conversation summaries with an optional cursor for the next page.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPage {
    pub items: Vec<ConversationSummary>,
    pub next_cursor: Option<ConversationCursor>,
}

/// Full conversation detail with all messages expanded.
///
/// @spec docs/L1-api#conversations-and-messages
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationView {
    pub id: ConversationId,
    pub subject: Option<String>,
    pub messages: Vec<MessageSummary>,
}

/// An account with its mailboxes, as rendered in the sidebar.
///
/// @spec docs/L1-api#navigation
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SidebarSource {
    pub id: AccountId,
    pub name: String,
    pub mailboxes: Vec<MailboxSummary>,
}

/// Distinguishes built-in smart mailboxes from user-created ones.
///
/// @spec docs/L1-accounts#smart-mailbox-defaults
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SmartMailboxKind {
    Default,
    User,
}

/// Smart mailbox entry with live counts for the sidebar.
///
/// @spec docs/L1-api#navigation
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SidebarSmartMailbox {
    pub id: SmartMailboxId,
    pub name: String,
    pub unread_messages: i64,
    pub total_messages: i64,
}

/// User-facing tag derived from non-system JMAP keywords.
///
/// @spec docs/L1-api#navigation
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagSummary {
    pub name: String,
    pub unread_messages: i64,
    pub total_messages: i64,
}

/// Combined sidebar payload: smart mailboxes at the top, then per-source mailboxes.
///
/// @spec docs/L1-api#navigation
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SidebarResponse {
    pub smart_mailboxes: Vec<SidebarSmartMailbox>,
    pub tags: Vec<TagSummary>,
    pub sources: Vec<SidebarSource>,
}

/// Boolean combinator for smart mailbox rule groups: `All` (AND) or `Any` (OR).
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SmartMailboxGroupOperator {
    All,
    Any,
}

/// Message field that a smart mailbox condition can filter on.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SmartMailboxField {
    SourceId,
    SourceName,
    MessageId,
    ThreadId,
    MailboxId,
    MailboxName,
    MailboxRole,
    IsRead,
    IsFlagged,
    HasAttachment,
    Keyword,
    FromName,
    FromEmail,
    Subject,
    Preview,
    ReceivedAt,
}

/// Comparison operator for a smart mailbox condition.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SmartMailboxOperator {
    Equals,
    In,
    Contains,
    Before,
    After,
    OnOrBefore,
    OnOrAfter,
}

/// Condition value: scalar string, string list (for `In`), or boolean.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SmartMailboxValue {
    String(String),
    Strings(Vec<String>),
    Bool(bool),
}

/// Boolean group node containing child conditions or nested groups.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartMailboxGroup {
    pub operator: SmartMailboxGroupOperator,
    pub negated: bool,
    pub nodes: Vec<SmartMailboxRuleNode>,
}

/// Leaf condition matching a single field with an operator and value.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartMailboxCondition {
    pub field: SmartMailboxField,
    pub operator: SmartMailboxOperator,
    pub negated: bool,
    pub value: SmartMailboxValue,
}

/// Recursive rule tree node: either a [`SmartMailboxGroup`] or a [`SmartMailboxCondition`].
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SmartMailboxRuleNode {
    Group(SmartMailboxGroup),
    Condition(SmartMailboxCondition),
}

/// Top-level rule for a smart mailbox, wrapping a root group.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartMailboxRule {
    pub root: SmartMailboxGroup,
}

/// A saved query with display metadata that behaves like a virtual mailbox.
///
/// @spec docs/L0-search#smart-mailboxes
/// @spec docs/L1-accounts#smart-mailbox-defaults
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartMailbox {
    pub id: SmartMailboxId,
    pub name: String,
    pub position: i64,
    pub kind: SmartMailboxKind,
    /// Identifies built-in smart mailboxes (e.g. "inbox", "trash").
    pub default_key: Option<String>,
    pub parent_id: Option<SmartMailboxId>,
    pub rule: SmartMailboxRule,
    pub created_at: String,
    pub updated_at: String,
}

/// Smart mailbox config with live unread/total counts from the store.
///
/// @spec docs/L1-api#smart-mailboxes
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartMailboxSummary {
    pub id: SmartMailboxId,
    pub name: String,
    pub position: i64,
    pub kind: SmartMailboxKind,
    pub default_key: Option<String>,
    pub parent_id: Option<SmartMailboxId>,
    pub unread_messages: i64,
    pub total_messages: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Mailbox state from a JMAP sync response, used in [`SyncBatch`].
///
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxRecord {
    pub id: MailboxId,
    pub name: String,
    pub role: Option<String>,
    pub unread_emails: i64,
    pub total_emails: i64,
}

/// Full email record from a JMAP sync response, used in [`SyncBatch`].
///
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRecord {
    pub id: MessageId,
    pub source_thread_id: ThreadId,
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
    /// RFC 2822 `Message-ID` header, used for threading.
    pub rfc_message_id: Option<String>,
    /// RFC 2822 `In-Reply-To` header, used for threading.
    pub in_reply_to: Option<String>,
    /// RFC 2822 `References` header chain, used for threading.
    pub references: Vec<String>,
}

/// Builds a minimal RFC 2822 message from constituent parts for draft storage.
///
/// @spec docs/L1-compose#mime-structures
pub fn synthesize_plain_text_raw_mime(
    from_header: &str,
    subject: &str,
    body_text: Option<&str>,
) -> String {
    format!(
        "From: {from_header}\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n",
        body_text.unwrap_or("")
    )
}

/// Returns the current UTC time formatted as an RFC 3339 string.
pub fn now_iso8601() -> Result<String, String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|err| err.to_string())
}

/// Atomic unit of sync data applied within a single SQLite transaction.
///
/// When a `replace_all_*` flag is true, the store treats that object list as a
/// full snapshot and prunes any local objects not present in the batch.
///
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncBatch {
    pub mailboxes: Vec<MailboxRecord>,
    pub messages: Vec<MessageRecord>,
    pub deleted_mailbox_ids: Vec<MailboxId>,
    pub deleted_message_ids: Vec<MessageId>,
    /// When true, mailboxes are a full snapshot (from full resync fallback).
    pub replace_all_mailboxes: bool,
    /// When true, messages are a full snapshot (from full resync fallback).
    pub replace_all_messages: bool,
    pub cursors: Vec<SyncCursor>,
}

/// Lazily-fetched message body content returned by the gateway.
///
/// @spec docs/L1-sync#sync-loop
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchedBody {
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub raw_mime: Option<String>,
    pub attachments: Vec<MessageAttachment>,
}

/// An ordered domain event stored in `event_log` and published via SSE.
///
/// @spec docs/L1-sync#event-propagation
/// @spec docs/L1-api#sse-event-stream
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

/// Query parameters for filtering the event log, used by `GET /v1/events`.
///
/// @spec docs/L1-api#sse-event-stream
#[derive(Clone, Debug)]
pub struct EventFilter {
    pub account_id: Option<AccountId>,
    pub topic: Option<String>,
    pub mailbox_id: Option<MailboxId>,
    pub after_seq: Option<i64>,
}

/// What caused a sync cycle to run.
///
/// @spec docs/L1-sync#sync-loop
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

/// A JMAP `StateChange` notification delivered over WebSocket or SSE.
///
/// @spec docs/L1-jmap#push
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushNotification {
    pub account_id: AccountId,
    pub changed: Vec<String>,
    pub received_at: String,
    /// Last-event-ID or push state for reconnection catch-up.
    pub checkpoint: Option<String>,
}

/// Async stream of push notifications from a single transport connection.
///
/// @spec docs/L1-jmap#push
pub type PushStream = Pin<Box<dyn Stream<Item = Result<PushNotification, GatewayError>> + Send>>;

/// Command to add and/or remove JMAP keywords on a message.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetKeywordsCommand {
    pub add: Vec<String>,
    pub remove: Vec<String>,
}

/// Command to atomically replace all mailbox memberships for a message.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceMailboxesCommand {
    pub mailbox_ids: Vec<MailboxId>,
}

/// Command to add a message to a single additional mailbox.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddToMailboxCommand {
    pub mailbox_id: MailboxId,
}

/// Command to remove a message from a single mailbox.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveFromMailboxCommand {
    pub mailbox_id: MailboxId,
}

/// Result of a message mutation: updated detail (if applicable) and emitted events.
///
/// @spec docs/L1-api#message-commands
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandResult {
    pub detail: Option<MessageDetail>,
    pub events: Vec<DomainEvent>,
}

/// Server-side outcome of a gateway mutation, carrying an updated sync cursor.
///
/// @spec docs/L1-sync#conflict-model
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationOutcome {
    pub cursor: Option<SyncCursor>,
}

/// JMAP sender identity for an account.
///
/// @spec docs/L1-jmap#core-types
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub email: String,
}

/// Email address with optional display name.
///
/// @spec docs/L1-jmap#methods-used
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipient {
    pub name: Option<String>,
    pub email: String,
}

/// Pre-computed reply/forward metadata fetched from the gateway.
///
/// @spec docs/L1-jmap#methods-used
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

/// Request payload for sending a new email via `EmailSubmission/set`.
///
/// @spec docs/L1-jmap#methods-used
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

/// Errors from JMAP gateway operations.
///
/// @spec docs/L1-jmap#error-model
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

/// Errors from the local SQLite store.
///
/// @spec docs/L1-sync#error-handling
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("storage failure: {0}")]
    Failure(String),
}

/// Unified error type surfaced by [`crate::MailService`] and mapped to HTTP status codes.
///
/// @spec docs/L1-api#error-format
#[derive(Debug, Error)]
pub enum ServiceError {
    #[error(transparent)]
    Gateway(#[from] GatewayError),
    #[error(transparent)]
    Secret(#[from] SecretStoreError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Config(#[from] crate::ConfigError),
}

impl ServiceError {
    /// Returns the error code string used in the JSON error response body.
    ///
    /// @spec docs/L1-api#error-code-mapping
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
            Self::Config(ConfigError::NotFound(_)) => "not_found",
            Self::Config(ConfigError::Conflict(_)) => "conflict",
            Self::Config(ConfigError::Validation(_)) => "config_validation",
            Self::Config(ConfigError::Io(_)) => "config_io",
            Self::Config(ConfigError::Parse(_)) => "config_parse",
        }
    }
}

/// Errors from credential storage operations.
///
/// @spec docs/L1-api#secret-management
#[derive(Debug, Error)]
pub enum SecretStoreError {
    #[error("secret unavailable: {0}")]
    Unavailable(String),
    #[error("secret store does not support operation: {0}")]
    Unsupported(String),
}
