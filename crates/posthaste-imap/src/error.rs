use thiserror::Error;

/// Errors raised by the IMAP adapter before they are mapped to domain gateway
/// errors by the account runtime.
#[derive(Debug, Error)]
pub enum ImapAdapterError {
    #[error("missing IMAP transport settings")]
    MissingTransport,
    #[error("missing SMTP transport settings")]
    MissingSmtpTransport,
    #[error("missing IMAP username")]
    MissingUsername,
    #[error("missing concrete SMTP sender email; configure email_patterns with an address")]
    MissingSmtpSenderEmail,
    #[error("missing IMAP secret")]
    MissingSecret,
    #[error("IMAP authentication error: {0}")]
    Auth(String),
    #[error("IMAP client error: {0}")]
    Client(String),
    #[error("invalid IMAP mailbox name: {0}")]
    InvalidMailboxName(String),
    #[error("IMAP SELECT/EXAMINE response missing {0}")]
    MissingSelectData(&'static str),
    #[error("IMAP UIDVALIDITY changed for {mailbox_name}: expected {expected}, got {actual}")]
    UidValidityMismatch {
        mailbox_name: String,
        expected: u32,
        actual: u32,
    },
    #[error("IMAP FETCH response missing {0}")]
    MissingFetchData(&'static str),
    #[error("invalid IMAP UID sequence set: {0}")]
    InvalidUidSequence(String),
    #[error("invalid IMAP MODSEQ: {0}")]
    InvalidModSeq(u64),
    #[error("invalid IMAP keyword flag {keyword}: {reason}")]
    InvalidKeywordFlag { keyword: String, reason: String },
    #[error("missing IMAP message location for mailbox {0}")]
    MissingMessageLocation(String),
    #[error("invalid IMAP blob id: {0}")]
    InvalidBlobId(String),
    #[error("could not parse RFC 5322 message headers")]
    ParseMessageHeaders,
    #[error("could not parse RFC 5322 message body")]
    ParseMessageBody,
    #[error("IMAP attachment {attachment_index} is missing from message {message_id}")]
    MissingAttachment {
        message_id: String,
        attachment_index: usize,
    },
    #[error("invalid SMTP email address {address}: {reason}")]
    InvalidSmtpAddress { address: String, reason: String },
    #[error("could not build SMTP message: {0}")]
    BuildSmtpMessage(String),
    #[error("SMTP transport error: {0}")]
    Smtp(String),
    /// A **send** whose delivery outcome is unknown: the SMTP transport dropped
    /// at or after the message body (DATA + terminating `.`) was written, so the
    /// MTA may already have accepted it. Kept distinct from [`Self::Smtp`] (a
    /// provably pre-write connect/greeting failure, safe to retry) so the outbox
    /// parks it as dispatch-uncertain and NEVER blind-resends — the duplicate-send
    /// fix (O5 at-most-once-on-uncertainty / RFC-L2 D86).
    #[error("SMTP send dispatch uncertain: {0}")]
    SmtpDispatchUncertain(String),
    #[error("IMAP {operation} did not complete within the deadline")]
    Timeout { operation: &'static str },
}

impl From<imap_client::client::tokio::ClientError> for ImapAdapterError {
    fn from(error: imap_client::client::tokio::ClientError) -> Self {
        Self::Client(error.to_string())
    }
}

impl From<lettre::error::Error> for ImapAdapterError {
    fn from(error: lettre::error::Error) -> Self {
        Self::BuildSmtpMessage(error.to_string())
    }
}

impl From<lettre::transport::smtp::Error> for ImapAdapterError {
    fn from(error: lettre::transport::smtp::Error) -> Self {
        Self::Smtp(error.to_string())
    }
}
