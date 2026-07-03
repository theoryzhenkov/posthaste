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
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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
    /// Stable client-assigned draft identity (`X-Posthaste-Draft-Id`), present
    /// only on drafts this client saved. Survives the provider id rotation that
    /// JMAP's immutable-draft update (create-new + destroy-old) causes, so a
    /// resumed draft is keyed by this instead of the rotating provider id.
    ///
    /// @spec docs/L1-outbox#temp-id-reconciliation
    #[serde(default)]
    pub draft_id: Option<String>,
}

/// Builds a minimal RFC 2822 message from constituent parts for draft storage.
///
/// @spec docs/L1-compose#mime-structures
pub fn synthesize_plain_text_raw_mime(
    from_header: &str,
    subject: &str,
    body_text: Option<&str>,
) -> String {
    synthesize_plain_text_raw_mime_with_recipients(
        Some(from_header),
        &[],
        &[],
        &[],
        subject,
        body_text,
    )
}

/// Builds a minimal RFC 2822 message including compose-recipient headers.
///
/// Used for provider APIs (notably JMAP) that fetch structured body fields but
/// do not provide raw RFC822 bytes. Preserving Cc/Bcc here lets draft resumption
/// reconstruct the compose form from the cached raw MIME.
///
/// @spec docs/L1-compose#mime-structures
pub fn synthesize_plain_text_raw_mime_with_recipients(
    from_header: Option<&str>,
    to: &[Recipient],
    cc: &[Recipient],
    bcc: &[Recipient],
    subject: &str,
    body_text: Option<&str>,
) -> String {
    let mut headers = String::new();
    if let Some(from_header) = from_header.filter(|value| !value.trim().is_empty()) {
        headers.push_str(&format!("From: {from_header}\r\n"));
    }
    if let Some(value) = recipients_to_header(to) {
        headers.push_str(&format!("To: {value}\r\n"));
    }
    if let Some(value) = recipients_to_header(cc) {
        headers.push_str(&format!("Cc: {value}\r\n"));
    }
    if let Some(value) = recipients_to_header(bcc) {
        headers.push_str(&format!("Bcc: {value}\r\n"));
    }
    headers.push_str(&format!(
        "Subject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n",
        body_text.unwrap_or("")
    ));
    headers
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
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
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
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventFilter {
    pub account_id: Option<AccountId>,
    pub topic: Option<String>,
    pub mailbox_id: Option<MailboxId>,
    pub after_seq: Option<i64>,
}

/// The seq bounds of the durable `event_log` — the oldest still-retained seq and
/// the highest assigned seq. Answers "where does replay start, and where is the
/// live head" in one cheap query (`MIN(seq)`/`MAX(seq)`), so the fact-carrying
/// tap resolves its gap frame and fresh-attach cursor without scanning the log
/// (RFC-L2-scripting D52 / S2: the cheap head query behind
/// `FactLog::highest_seq`/`truncation_point`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventLogBounds {
    /// The oldest seq still retained (the truncation point). A resume from before
    /// it cannot be served from durable history.
    pub oldest: i64,
    /// The highest seq assigned (the live head a fresh subscriber attaches at).
    pub newest: i64,
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
