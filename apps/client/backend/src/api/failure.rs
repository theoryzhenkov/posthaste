//! The one failure shape of the API layer: a status code plus the models
//! error envelope, and the mappings from service/store errors onto it.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use posthaste_client_models::{ApiError, ApiErrorKind};
use posthaste_domain_model::{ServiceError, ServiceErrorKind};

/// A failed HTTP call: a status code plus the one models error envelope.
#[derive(Debug)]
pub(crate) struct ApiFailure {
    pub(crate) status: StatusCode,
    pub(crate) error: ApiError,
}

impl ApiFailure {
    pub(crate) fn new(
        status: StatusCode,
        kind: ApiErrorKind,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self {
            status,
            error: ApiError {
                kind,
                message: message.into(),
                retryable,
            },
        }
    }

    pub(crate) fn malformed(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            ApiErrorKind::MalformedRequest,
            message,
            false,
        )
    }

    pub(crate) fn unknown_id(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            ApiErrorKind::UnknownId,
            message,
            false,
        )
    }

    pub(crate) fn unavailable(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            ApiErrorKind::Unavailable,
            message,
            true,
        )
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorKind::Internal,
            message,
            false,
        )
    }
}

impl IntoResponse for ApiFailure {
    fn into_response(self) -> Response {
        (self.status, Json(self.error)).into_response()
    }
}

impl From<posthaste_domain_model::StoreError> for ApiFailure {
    fn from(error: posthaste_domain_model::StoreError) -> Self {
        Self::from(ServiceError::from(error))
    }
}

impl From<ServiceError> for ApiFailure {
    fn from(error: ServiceError) -> Self {
        let message = error.to_string();
        match error.kind() {
            ServiceErrorKind::NotFound => Self::unknown_id(message),
            ServiceErrorKind::Conflict | ServiceErrorKind::MailboxNotEmpty => {
                Self::new(StatusCode::CONFLICT, ApiErrorKind::Conflict, message, false)
            }
            ServiceErrorKind::ConfigValidation | ServiceErrorKind::ConfigParse => {
                Self::malformed(message)
            }
            ServiceErrorKind::GatewayUnavailable
            | ServiceErrorKind::NetworkError
            | ServiceErrorKind::SecretUnavailable => Self::unavailable(message),
            ServiceErrorKind::AuthError
            | ServiceErrorKind::StateMismatch
            | ServiceErrorKind::CannotCalculateChanges
            | ServiceErrorKind::GatewayRejected => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                ApiErrorKind::Unavailable,
                message,
                false,
            ),
            ServiceErrorKind::StorageFailure | ServiceErrorKind::ConfigIo => Self::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiErrorKind::Internal,
                message,
                true,
            ),
            ServiceErrorKind::StorageCorrupted
            | ServiceErrorKind::SecretUnsupported
            | ServiceErrorKind::Internal => Self::internal(message),
        }
    }
}
