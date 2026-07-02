use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationError {
    InvalidAccount(String),
    BaseUrlRequired(String),
    SecretRequired(String),
    UsernameRequired(String),
    SenderRequired(String),
    DuplicateSourceId(String),
    DuplicateSmartMailboxId(String),
    DanglingDefaultAccount(String),
}

impl ValidationError {
    pub fn message(&self) -> &str {
        match self {
            Self::InvalidAccount(message)
            | Self::BaseUrlRequired(message)
            | Self::SecretRequired(message)
            | Self::UsernameRequired(message)
            | Self::SenderRequired(message) => message.as_str(),
            Self::DuplicateSourceId(_) => "duplicate source id",
            Self::DuplicateSmartMailboxId(_) => "duplicate smart mailbox id",
            Self::DanglingDefaultAccount(_) => "default account must reference an existing account",
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAccount(message)
            | Self::BaseUrlRequired(message)
            | Self::SecretRequired(message)
            | Self::UsernameRequired(message)
            | Self::SenderRequired(message) => formatter.write_str(message),
            Self::DuplicateSourceId(id) => write!(formatter, "duplicate source id '{id}'"),
            Self::DuplicateSmartMailboxId(id) => {
                write!(formatter, "duplicate smart mailbox id '{id}'")
            }
            Self::DanglingDefaultAccount(id) => {
                write!(formatter, "default account '{id}' does not exist")
            }
        }
    }
}
