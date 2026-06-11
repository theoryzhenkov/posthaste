use super::*;

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
    #[serde(default)]
    pub to: Vec<Recipient>,
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
    pub imap_mailbox_states: Vec<ImapMailboxSyncState>,
    pub imap_message_locations: Vec<ImapMessageLocation>,
    /// IMAP location keys that disappeared from a mailbox-scoped delta.
    ///
    /// This is distinct from `deleted_message_ids`: one vanished IMAP UID can
    /// mean a Gmail label was removed while the canonical message still exists
    /// in another mailbox location.
    pub deleted_imap_message_locations: Vec<ImapMessageLocationKey>,
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
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DomainEvent {
    pub seq: i64,
    pub account_id: AccountId,
    pub topic: String,
    pub occurred_at: String,
    pub mailbox_id: Option<MailboxId>,
    pub message_id: Option<MessageId>,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
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
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
