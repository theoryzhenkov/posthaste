use thiserror::Error;

/// Errors raised by the IMAP adapter before they are mapped to domain gateway
/// errors by the account runtime.
#[derive(Debug, Error)]
pub enum ImapAdapterError {
    #[error("missing IMAP transport settings")]
    MissingTransport,
    #[error("missing IMAP username")]
    MissingUsername,
    #[error("missing IMAP secret")]
    MissingSecret,
    #[error("IMAP client error: {0}")]
    Client(String),
    #[error("invalid IMAP mailbox name: {0}")]
    InvalidMailboxName(String),
    #[error("IMAP SELECT/EXAMINE response missing {0}")]
    MissingSelectData(&'static str),
    #[error("IMAP FETCH response missing {0}")]
    MissingFetchData(&'static str),
    #[error("invalid IMAP UID sequence set: {0}")]
    InvalidUidSequence(String),
    #[error("could not parse RFC 5322 message headers")]
    ParseMessageHeaders,
    #[error("could not parse RFC 5322 message body")]
    ParseMessageBody,
}

impl From<imap_client::client::tokio::ClientError> for ImapAdapterError {
    fn from(error: imap_client::client::tokio::ClientError) -> Self {
        Self::Client(error.to_string())
    }
}
