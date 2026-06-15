use super::*;

/// Stable machine-readable API error code.
///
/// The single typed code space for the `/v1` surface: boundary-validation codes
/// raised by the API layer, plus the domain [`ServiceErrorKind`] codes mapped via
/// [`From<ServiceErrorKind>`]. Serializes to snake_case wire strings.
///
/// @spec docs/L1-api#error-format
/// @spec docs/L1-api#error-code-mapping
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApiErrorCode {
    // Boundary validation.
    InvalidQuery,
    InvalidCursor,
    InvalidLimit,
    InvalidMailbox,
    InvalidCompose,
    InvalidSecret,
    InvalidProvider,
    InvalidAccount,
    InvalidAccountLogo,
    InvalidOauthRequest,
    InvalidOauthCallback,
    // OAuth outcomes.
    OauthDenied,
    InvalidGrant,
    // Account-field validation (the split of the old generic `invalid_account`).
    AccountBaseUrlRequired,
    AccountSecretRequired,
    AccountUsernameRequired,
    AccountSenderRequired,
    // Generic.
    NotFound,
    Conflict,
    InternalError,
    // Authentication / authorization (loopback trust model, default-off).
    Unauthorized,
    Forbidden,
    // Domain (mapped from `ServiceErrorKind`).
    GatewayUnavailable,
    AuthError,
    NetworkError,
    StateMismatch,
    CannotCalculateChanges,
    GatewayRejected,
    SecretUnavailable,
    SecretUnsupported,
    StorageFailure,
    ConfigValidation,
    ConfigIo,
    ConfigParse,
}

impl From<ServiceErrorKind> for ApiErrorCode {
    fn from(kind: ServiceErrorKind) -> Self {
        match kind {
            ServiceErrorKind::GatewayUnavailable => Self::GatewayUnavailable,
            ServiceErrorKind::AuthError => Self::AuthError,
            ServiceErrorKind::NetworkError => Self::NetworkError,
            ServiceErrorKind::StateMismatch => Self::StateMismatch,
            ServiceErrorKind::CannotCalculateChanges => Self::CannotCalculateChanges,
            ServiceErrorKind::GatewayRejected => Self::GatewayRejected,
            ServiceErrorKind::SecretUnavailable => Self::SecretUnavailable,
            ServiceErrorKind::SecretUnsupported => Self::SecretUnsupported,
            ServiceErrorKind::NotFound => Self::NotFound,
            ServiceErrorKind::Conflict => Self::Conflict,
            ServiceErrorKind::StorageFailure => Self::StorageFailure,
            ServiceErrorKind::ConfigValidation => Self::ConfigValidation,
            ServiceErrorKind::ConfigIo => Self::ConfigIo,
            ServiceErrorKind::ConfigParse => Self::ConfigParse,
        }
    }
}

/// JSON error response body returned by all API error paths.
///
/// @spec docs/L1-api#error-format
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorBody {
    /// Stable machine-readable error code.
    pub code: ApiErrorCode,
    /// Human-readable description of the failure.
    pub message: String,
    /// Optional structured context for the error.
    #[schema(value_type = Object)]
    pub details: serde_json::Value,
}

/// Structured API error carrying an HTTP status code and a JSON body.
///
/// @spec docs/L1-api#error-format
/// @spec docs/L1-api#error-code-mapping
pub struct ApiError {
    pub(crate) status: StatusCode,
    pub(crate) body: ApiErrorBody,
}

impl ApiError {
    /// Map a domain `ServiceError` to an HTTP status code and JSON error body.
    ///
    /// @spec docs/L1-api#error-code-mapping
    pub fn from_service_error(error: ServiceError) -> Self {
        let status = service_error_status(error.kind());
        Self {
            status,
            body: ApiErrorBody {
                code: ApiErrorCode::from(error.kind()),
                message: error.to_string(),
                details: json!({}),
            },
        }
    }

    /// Map a runtime-contract error to the `/v1` error envelope.
    ///
    /// @spec docs/backend/L4#runtime-error-to-api
    pub fn from_runtime_error(error: RuntimeError) -> Self {
        let envelope = error.envelope();
        let (status, code) = runtime_error_status_code(&envelope.code);
        Self {
            status,
            body: ApiErrorBody {
                code,
                message: envelope.message.clone(),
                details: envelope.details.clone(),
            },
        }
    }

    /// Construct an `ApiError` with explicit status, code, and message.
    pub fn new(status: StatusCode, code: ApiErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,
            body: ApiErrorBody {
                code,
                message: message.into(),
                details: json!({}),
            },
        }
    }
}

fn runtime_error_status_code(code: &RuntimeErrorCode) -> (StatusCode, ApiErrorCode) {
    match code {
        RuntimeErrorCode::RuntimeNotReady => {
            (StatusCode::SERVICE_UNAVAILABLE, ApiErrorCode::InternalError)
        }
        RuntimeErrorCode::InvalidDescriptor | RuntimeErrorCode::InvalidMutation => {
            (StatusCode::BAD_REQUEST, ApiErrorCode::InvalidQuery)
        }
        RuntimeErrorCode::InvalidSecret => (StatusCode::BAD_REQUEST, ApiErrorCode::InvalidSecret),
        RuntimeErrorCode::InvalidAccount => (StatusCode::BAD_REQUEST, ApiErrorCode::InvalidAccount),
        RuntimeErrorCode::AccountBaseUrlRequired => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::AccountBaseUrlRequired,
        ),
        RuntimeErrorCode::AccountSecretRequired => {
            (StatusCode::BAD_REQUEST, ApiErrorCode::AccountSecretRequired)
        }
        RuntimeErrorCode::AccountUsernameRequired => (
            StatusCode::BAD_REQUEST,
            ApiErrorCode::AccountUsernameRequired,
        ),
        RuntimeErrorCode::AccountSenderRequired => {
            (StatusCode::BAD_REQUEST, ApiErrorCode::AccountSenderRequired)
        }
        RuntimeErrorCode::Unauthorized => (StatusCode::UNAUTHORIZED, ApiErrorCode::Unauthorized),
        RuntimeErrorCode::NotFound => (StatusCode::NOT_FOUND, ApiErrorCode::NotFound),
        RuntimeErrorCode::ProviderUnavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            ApiErrorCode::GatewayUnavailable,
        ),
        RuntimeErrorCode::Conflict => (StatusCode::CONFLICT, ApiErrorCode::Conflict),
        RuntimeErrorCode::NetworkError => (StatusCode::BAD_GATEWAY, ApiErrorCode::NetworkError),
        RuntimeErrorCode::StateMismatch => (StatusCode::CONFLICT, ApiErrorCode::StateMismatch),
        RuntimeErrorCode::GatewayRejected => {
            (StatusCode::BAD_REQUEST, ApiErrorCode::GatewayRejected)
        }
        RuntimeErrorCode::SecretUnavailable => {
            (StatusCode::BAD_REQUEST, ApiErrorCode::SecretUnavailable)
        }
        RuntimeErrorCode::SecretUnsupported => {
            (StatusCode::BAD_REQUEST, ApiErrorCode::SecretUnsupported)
        }
        RuntimeErrorCode::ConfigValidation => {
            (StatusCode::BAD_REQUEST, ApiErrorCode::ConfigValidation)
        }
        RuntimeErrorCode::ConfigParse => (StatusCode::BAD_REQUEST, ApiErrorCode::ConfigParse),
        RuntimeErrorCode::CannotCalculateChanges => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::CannotCalculateChanges,
        ),
        RuntimeErrorCode::StorageFailure => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::StorageFailure,
        ),
        RuntimeErrorCode::ConfigIo => (StatusCode::INTERNAL_SERVER_ERROR, ApiErrorCode::ConfigIo),
        RuntimeErrorCode::TransportDisconnected | RuntimeErrorCode::Internal => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::InternalError,
        ),
    }
}

fn service_error_status(kind: ServiceErrorKind) -> StatusCode {
    match kind {
        ServiceErrorKind::NotFound => StatusCode::NOT_FOUND,
        ServiceErrorKind::Conflict | ServiceErrorKind::StateMismatch => StatusCode::CONFLICT,
        ServiceErrorKind::AuthError => StatusCode::UNAUTHORIZED,
        ServiceErrorKind::GatewayUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        ServiceErrorKind::NetworkError => StatusCode::BAD_GATEWAY,
        ServiceErrorKind::GatewayRejected
        | ServiceErrorKind::SecretUnavailable
        | ServiceErrorKind::SecretUnsupported
        | ServiceErrorKind::ConfigValidation
        | ServiceErrorKind::ConfigParse => StatusCode::BAD_REQUEST,
        ServiceErrorKind::CannotCalculateChanges
        | ServiceErrorKind::StorageFailure
        | ServiceErrorKind::ConfigIo => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl From<ServiceError> for ApiError {
    fn from(error: ServiceError) -> Self {
        Self::from_service_error(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}
