//! The message-detail family: one message opened for reading, and its full
//! RFC 822 source.

use posthaste_domain_model as domain;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Read one message with its body content.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetailQuery {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MessageId")]
    pub message_id: domain::MessageId,
}

/// One message opened for reading: its summary row plus sanitized bodies and
/// attachment metadata (attachment bytes are fetched as blobs).
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetailResult {
    #[ts(as = "crate::mirror::MessageSummary")]
    pub summary: domain::MessageSummary,
    pub body_html: Option<String>,
    pub body_text: Option<String>,
    #[ts(as = "Vec<crate::mirror::MessageAttachment>")]
    pub attachments: Vec<domain::MessageAttachment>,
    /// Unsubscribe targets parsed from the message headers, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<crate::mirror::ListUnsubscribe>")]
    pub list_unsubscribe: Option<domain::ListUnsubscribe>,
}

/// Read one message's full unparsed RFC 822 source (the "view source" /
/// "download .eml" surface).
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MessageRawSourceQuery {
    #[ts(as = "crate::mirror::AccountId")]
    pub account_id: domain::AccountId,
    #[ts(as = "crate::mirror::MessageId")]
    pub message_id: domain::MessageId,
}

/// The full RFC 822 source of one message, verbatim.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MessageRawSourceResult {
    /// The complete raw message (headers and body) as transmitted.
    pub raw: String,
}
