use super::*;

/// Metadata for a locally-cached raw MIME message file.
///
/// @spec docs/L1-sync#sync-loop
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
    /// Per-message authority-state version (IMAP per-message `max(modseq)`);
    /// `None` for providers without a per-message version (JMAP, mock/local).
    /// The client replica uses it as a staleness guard on base ingest: a base
    /// whose `version` is strictly older than the held one is rejected, so a
    /// late provider re-serve can't clobber a confirmed optimistic mutation
    /// (flicker Bug 1b). Absent ⇒ unguarded.
    /// @spec docs/eph/DESIGN-L2-message-authority-version
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u64>,
    /// This message's RFC822 `Message-ID`, when known. With [`in_reply_to`] it
    /// lets the conversation view build a real reply tree (match a reply's
    /// `in_reply_to` to its parent's `rfc_message_id`). `None` for providers/
    /// messages without one.
    ///
    /// [`in_reply_to`]: Self::in_reply_to
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rfc_message_id: Option<String>,
    /// The `Message-ID` this message is a reply to (its `In-Reply-To` header),
    /// i.e. the parent in the reply tree. `None` for thread roots / messages
    /// without the header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_reply_to: Option<String>,
}

/// Column by which message lists can be sorted.
///
/// @spec docs/L1-api#cursor-pagination
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConversationRef {
    pub conversation_id: ConversationId,
    pub conversation_token: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MessageSummaryState {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub conversation_ref: ConversationRef,
    pub body_token: Option<String>,
    pub attachment_token: Option<String>,
}

impl MessageSummaryState {
    pub fn from_summary(summary: MessageSummary) -> Self {
        let conversation_token = format!("conversation:{}", summary.conversation_id.as_str());
        Self {
            conversation_ref: ConversationRef {
                conversation_id: summary.conversation_id.clone(),
                conversation_token,
            },
            summary,
            body_token: None,
            attachment_token: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MessageChangeAssertion {
    pub before: Option<MessageSummaryState>,
    pub after: Option<MessageSummaryState>,
}

impl MessageChangeAssertion {
    pub fn after(summary: MessageSummary) -> Self {
        Self {
            before: None,
            after: Some(MessageSummaryState::from_summary(summary)),
        }
    }
}

/// Full message including sanitized body content, returned by message detail endpoint.
///
/// @spec docs/L1-api#message-body-sanitization
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MessageDetail {
    #[serde(flatten)]
    pub summary: MessageSummary,
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    pub raw_message: Option<RawMessageRef>,
    pub attachments: Vec<MessageAttachment>,
    /// Stable `X-Posthaste-Draft-Id` for this message when it is a draft this
    /// client saved; `None` otherwise.
    ///
    /// @spec docs/L1-outbox#temp-id-reconciliation
    #[serde(default)]
    pub draft_id: Option<String>,
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
