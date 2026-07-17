//! Session-secret authentication on every route.

use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use posthaste_client_models::ApiErrorKind;

use super::{ApiFailure, ApiState};

/// Session-secret check on every route. Accepts the bearer header
/// everywhere, and the `?token=` query parameter only on the GETs whose
/// consumers cannot set headers (the browser `EventSource` on `/events`,
/// plain anchors on blob and logo downloads) — so the credential stays out
/// of URLs on the query/command routes. The comparison is constant-time: the
/// token cannot be recovered byte-by-byte from response timing.
pub(crate) async fn require_auth(
    State(state): State<ApiState>,
    request: Request,
    next: Next,
) -> Response {
    let presented = bearer_token(request.headers()).or_else(|| {
        accepts_query_token(&request)
            .then(|| query_token(request.uri()))
            .flatten()
    });
    let authorized = presented
        .as_deref()
        .is_some_and(|token| constant_time_eq(token.as_bytes(), state.token.as_bytes()));
    if !authorized {
        return ApiFailure::new(
            StatusCode::UNAUTHORIZED,
            ApiErrorKind::Unauthorized,
            "missing or invalid session token",
            false,
        )
        .into_response();
    }
    next.run(request).await
}

/// The routes that accept `?token=`: `GET /events`, `GET /blobs/{id}`, and
/// `GET /account-assets/...` (with or without the `/api` prefix).
fn accepts_query_token(request: &Request) -> bool {
    if request.method() != Method::GET {
        return false;
    }
    let path = request.uri().path();
    let path = path.strip_prefix("/api").unwrap_or(path);
    path == "/events" || path.starts_with("/blobs/") || path.starts_with("/account-assets/")
}

/// Equality over every byte with no early exit. The comparison time leaks
/// only the length mismatch (the token length is fixed and public), never
/// which byte differs.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |acc, (l, r)| acc | (l ^ r))
        == 0
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::to_string)
}

fn query_token(uri: &Uri) -> Option<String> {
    uri.query()?
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_matches_plain_equality() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(constant_time_eq(b"", b""));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secre"));
        assert!(!constant_time_eq(b"", b"x"));
    }
}
