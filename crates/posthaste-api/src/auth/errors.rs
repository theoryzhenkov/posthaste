use axum::http::StatusCode;

use crate::api::{ApiError, ApiErrorCode};

pub fn unauthorized() -> ApiError {
    ApiError::new(
        StatusCode::UNAUTHORIZED,
        ApiErrorCode::Unauthorized,
        "missing or invalid bearer token",
    )
}

pub(crate) fn forbidden() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        ApiErrorCode::Forbidden,
        "request origin is not allowed",
    )
}

/// 403: the token is authentic but a caveat is not satisfied (out of scope).
pub(crate) fn forbidden_scope() -> ApiError {
    ApiError::new(
        StatusCode::FORBIDDEN,
        ApiErrorCode::Forbidden,
        "token is not authorized for this request",
    )
}

/// 500: a scoped token reached a route with no authz-map entry. Failing closed
/// here means a newly added, unmapped route denies attenuated tokens rather than
/// silently granting them.
pub(crate) fn misconfigured() -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        ApiErrorCode::InternalError,
        "route is not present in the authorization map",
    )
}
