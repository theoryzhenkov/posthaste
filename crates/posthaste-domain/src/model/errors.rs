use super::*;

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

/// Stable service error category for exhaustive API status mapping.
///
/// @spec docs/L1-api#error-code-mapping
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceErrorKind {
    GatewayUnavailable,
    AuthError,
    NetworkError,
    StateMismatch,
    CannotCalculateChanges,
    GatewayRejected,
    SecretUnavailable,
    SecretUnsupported,
    NotFound,
    Conflict,
    StorageFailure,
    ConfigValidation,
    ConfigIo,
    ConfigParse,
}

impl ServiceErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::GatewayUnavailable => "gateway_unavailable",
            Self::AuthError => "auth_error",
            Self::NetworkError => "network_error",
            Self::StateMismatch => "state_mismatch",
            Self::CannotCalculateChanges => "cannot_calculate_changes",
            Self::GatewayRejected => "gateway_rejected",
            Self::SecretUnavailable => "secret_unavailable",
            Self::SecretUnsupported => "secret_unsupported",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::StorageFailure => "storage_failure",
            Self::ConfigValidation => "config_validation",
            Self::ConfigIo => "config_io",
            Self::ConfigParse => "config_parse",
        }
    }
}

impl ServiceError {
    /// Returns the stable category used for API status mapping.
    ///
    /// @spec docs/L1-api#error-code-mapping
    pub fn kind(&self) -> ServiceErrorKind {
        match self {
            Self::Gateway(GatewayError::Unavailable(_)) => ServiceErrorKind::GatewayUnavailable,
            Self::Gateway(GatewayError::Auth) => ServiceErrorKind::AuthError,
            Self::Gateway(GatewayError::Network(_)) => ServiceErrorKind::NetworkError,
            Self::Gateway(GatewayError::StateMismatch) => ServiceErrorKind::StateMismatch,
            Self::Gateway(GatewayError::CannotCalculateChanges) => {
                ServiceErrorKind::CannotCalculateChanges
            }
            Self::Gateway(GatewayError::Rejected(_)) => ServiceErrorKind::GatewayRejected,
            Self::Secret(SecretStoreError::Unavailable(_)) => ServiceErrorKind::SecretUnavailable,
            Self::Secret(SecretStoreError::Unsupported(_)) => ServiceErrorKind::SecretUnsupported,
            Self::Store(StoreError::NotFound(_)) | Self::Config(ConfigError::NotFound(_)) => {
                ServiceErrorKind::NotFound
            }
            Self::Store(StoreError::Conflict(_)) | Self::Config(ConfigError::Conflict(_)) => {
                ServiceErrorKind::Conflict
            }
            Self::Store(StoreError::Failure(_)) => ServiceErrorKind::StorageFailure,
            Self::Config(ConfigError::Validation(_)) => ServiceErrorKind::ConfigValidation,
            Self::Config(ConfigError::Io(_)) => ServiceErrorKind::ConfigIo,
            Self::Config(ConfigError::Parse(_)) => ServiceErrorKind::ConfigParse,
        }
    }

    /// Returns the error code string used in the JSON error response body.
    ///
    /// @spec docs/L1-api#error-code-mapping
    pub fn code(&self) -> &'static str {
        self.kind().code()
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
