use super::*;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ImapUid(pub u32);

/// IMAP UIDVALIDITY value. If this changes, cached UIDs for the mailbox are no
/// longer valid.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ImapUidValidity(pub u32);

/// IMAP CONDSTORE/QRESYNC modification sequence.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ImapModSeq(pub u64);

/// Gmail's stable IMAP message identifier from `X-GM-MSGID`.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GmailMessageId(pub u64);

/// Gmail's stable IMAP thread identifier from `X-GM-THRID`.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GmailThreadId(pub u64);

/// Gmail label name from `X-GM-LABELS`.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct GmailLabel(pub String);

impl GmailLabel {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for GmailLabel {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for GmailLabel {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// Typed Gmail metadata carried by IMAP FETCH.
///
/// These values are present only when the protocol layer has requested and
/// parsed Gmail's `X-GM-*` FETCH extensions.
///
/// @spec docs/L0-providers#identity-and-threading
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImapGmailMetadata {
    pub message_id: Option<GmailMessageId>,
    pub thread_id: Option<GmailThreadId>,
    #[serde(default)]
    pub labels_observed: bool,
    pub labels: Vec<GmailLabel>,
}
