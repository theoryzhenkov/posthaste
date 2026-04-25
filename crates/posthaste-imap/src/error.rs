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
}

impl From<imap_client::client::tokio::ClientError> for ImapAdapterError {
    fn from(error: imap_client::client::tokio::ClientError) -> Self {
        Self::Client(error.to_string())
    }
}
