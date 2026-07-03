use super::*;
use posthaste_observability::{events, ph_error};

/// The stable, generic client-facing message for every sanitized 5xx body. The
/// real cause (io/sql/runtime `Display` text) never appears here — it is logged
/// server-side only (D72). The correlation id in `details.correlationId` joins
/// this body to its `HTTP_INTERNAL_ERROR` operator log line.
const INTERNAL_ERROR_MESSAGE: &str = "internal error";

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
    StorageCorrupted,
    ConfigValidation,
    ConfigIo,
    ConfigParse,
}

impl From<ServiceErrorKind> for ApiErrorCode {
    fn from(kind: ServiceErrorKind) -> Self {
        match kind {
            // An internal codec/logic fault surfaced through a gateway op.
            // M30: this is where boundary sanitization/operator-logging of the
            // 500 body will hook in; M29 only routes the class to the code.
            ServiceErrorKind::Internal => Self::InternalError,
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
            ServiceErrorKind::StorageCorrupted => Self::StorageCorrupted,
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
        let code = ApiErrorCode::from(error.kind());
        // D72 boundary line: an internal-class (5xx) `ServiceError` can carry
        // io/sql `Display` text — it is logged, then replaced with the generic
        // sanitized body. A 4xx message is caller-actionable validation and stays.
        if status.is_server_error() {
            return sanitized_internal_error(status, code, error);
        }
        Self {
            status,
            body: ApiErrorBody {
                code,
                message: error.to_string(),
                details: json!({}),
            },
        }
    }

    /// Map a runtime-contract error to the `/v1` error envelope.
    ///
    /// @spec docs/authority-server/L3#runtime-error-to-api
    pub fn from_runtime_error(error: RuntimeError) -> Self {
        // M30: the envelope's typed `terminality` (D70) is not yet surfaced on
        // the `/v1` `ApiErrorBody`; when the boundary codes land (D71/D72), emit
        // it here so a browser near-end's `from_response` can respect it instead
        // of falling back to the HTTP status band.
        let envelope = error.envelope();
        let (status, code) = runtime_error_status_code(&envelope.code);
        // D72 boundary line: a 5xx runtime envelope's `message`/`details` can
        // carry server-internal detail (storage/transport/internal-fault text) —
        // log it and return the generic body. 4xx runtime messages are
        // caller-actionable (invalid descriptor/mutation, missing account field)
        // and stay verbatim.
        if status.is_server_error() {
            return sanitized_internal_error(status, code, &envelope.message);
        }
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

/// Build a sanitized 5xx `ApiError`, the single boundary chokepoint for
/// internal-class errors (D72): mint a correlation id, log the real `cause` at
/// `error!` joined by that id, and return a body carrying only the generic
/// message + the correlation id. Server-internal detail (io/sql/runtime text) is
/// logged, never surfaced to the client.
pub(crate) fn sanitized_internal_error(
    status: StatusCode,
    code: ApiErrorCode,
    cause: impl std::fmt::Display,
) -> ApiError {
    debug_assert!(
        status.is_server_error(),
        "sanitized_internal_error is for 5xx only"
    );
    let correlation_id = Id::generate().to_string();
    ph_error!(
        events::HTTP_INTERNAL_ERROR,
        status = status.as_u16(),
        code = ?code,
        correlation_id = %correlation_id,
        cause = %cause,
        "internal error serving /v1 request"
    );
    ApiError {
        status,
        body: ApiErrorBody {
            code,
            message: INTERNAL_ERROR_MESSAGE.to_string(),
            details: json!({ "correlationId": correlation_id }),
        },
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
        RuntimeErrorCode::StorageCorrupted => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::StorageCorrupted,
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
        ServiceErrorKind::Internal
        | ServiceErrorKind::CannotCalculateChanges
        | ServiceErrorKind::StorageFailure
        | ServiceErrorKind::StorageCorrupted
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
