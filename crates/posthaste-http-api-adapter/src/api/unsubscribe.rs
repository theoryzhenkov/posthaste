//! RFC 8058 one-click unsubscribe execution.
//!
//! The renderer never performs the POST: an unsubscribe is an outbound side
//! effect in the user's name, so it is executed here, server-side, with a
//! locked-down client — https-only, no cookies, no auth, no redirect
//! downgrade, bounded timeout — and only for a message whose stored
//! `list_unsubscribe` targets were parsed (and validated) at ingest. The URL is
//! re-validated here anyway: stored data is treated as untrusted input.
//!
//! @spec docs/L1-api#message-commands

use std::sync::OnceLock;
use std::time::Duration;

use super::*;

/// RFC 8058 §3.2: the POST body is exactly this form-encoded pair.
pub(crate) const ONE_CLICK_BODY: &str = "List-Unsubscribe=One-Click";

/// Total deadline for the outbound POST (connect + response headers). The
/// remote is an arbitrary third-party list server; the user is waiting on a
/// dialog, so fail fast rather than hang.
const ONE_CLICK_TIMEOUT: Duration = Duration::from_secs(15);

/// Result of a successful one-click unsubscribe POST.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeAck {
    /// HTTP status the list server answered with (always 2xx here — a non-2xx
    /// answer is surfaced as an error). The response body is never surfaced.
    pub http_status: u16,
}

/// POST /v1/sources/{sid}/commands/messages/{mid}/unsubscribe
///
/// Executes the message's RFC 8058 one-click unsubscribe: POSTs
/// `List-Unsubscribe=One-Click` to the stored https target. Only available
/// when the message carries a one-click target; `mailto:` and plain-link
/// unsubscribes are client-mediated flows and never reach this endpoint.
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/commands/messages/{message_id}/unsubscribe",
    tag = "messages",
    summary = "One-click unsubscribe",
    description = "Performs the RFC 8058 one-click unsubscribe POST for this message's stored \
                   List-Unsubscribe target. Server-side, https-only, credential-free; the \
                   response body of the list server is never surfaced.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("message_id" = String, Path, description = "Message identifier")
    ),
    responses(
        (status = 200, description = "The list server acknowledged the unsubscribe", body = UnsubscribeAck),
        (status = 400, description = "The message has no valid one-click unsubscribe target", body = ApiErrorBody),
        (status = 404, description = "Message not found", body = ApiErrorBody),
        (status = 502, description = "The list server did not accept the unsubscribe", body = ApiErrorBody)
    )
)]
pub async fn unsubscribe_message(
    State(state): State<Arc<AppState>>,
    Path((source_id, message_id)): Path<(String, String)>,
) -> Result<Json<UnsubscribeAck>, ApiError> {
    let result = state
        .runtime
        .get_message_detail(
            RuntimeCaller::api(),
            AccountId(source_id),
            MessageId(message_id),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    let detail = result.detail.ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            ApiErrorCode::NotFound,
            "message detail not available",
        )
    })?;

    let Some(targets) = detail.list_unsubscribe else {
        return Err(unavailable("message has no unsubscribe target"));
    };
    if !targets.one_click {
        return Err(unavailable(
            "message has no one-click (RFC 8058) unsubscribe target",
        ));
    }
    let Some(https) = targets.https else {
        return Err(unavailable("message has no https unsubscribe target"));
    };
    let url = validated_one_click_url(&https)?;

    let http_status = post_one_click(one_click_client(), url, ONE_CLICK_TIMEOUT).await?;
    Ok(Json(UnsubscribeAck { http_status }))
}

fn unavailable(message: &str) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        ApiErrorCode::UnsubscribeUnavailable,
        message,
    )
}

/// Server-side re-validation of the stored https target — stored data is
/// untrusted input. Both the shared conservative validator (the one ingest
/// used) and a full URL parse must agree: https scheme, no userinfo, a DNS-name
/// host (never an IP literal).
fn validated_one_click_url(raw: &str) -> Result<url::Url, ApiError> {
    let reject = || unavailable("stored unsubscribe target is not a valid https URL");
    posthaste_domain_model::validate_one_click_https(raw).map_err(|_| reject())?;
    let url = url::Url::parse(raw).map_err(|_| reject())?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.host(), Some(url::Host::Domain(_)))
    {
        return Err(reject());
    }
    Ok(url)
}

/// The locked-down outbound client for one-click POSTs, built once. No cookie
/// store (reqwest default), no auth, no default headers; https-only; redirects
/// followed only https→https and at most 3 hops.
fn one_click_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| build_one_click_client(true))
}

/// `https_only(false)` exists for the transport tests only (a loopback mock
/// cannot serve TLS); the production client is always built with `true`.
pub(crate) fn build_one_click_client(https_only: bool) -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(one_click_redirect_policy())
        .https_only(https_only)
        .connect_timeout(Duration::from_secs(10))
        .build()
        .expect("static one-click client config should build")
}

/// Follow at most 3 redirects, and only to https targets — a redirect that
/// downgrades to http aborts the request.
fn one_click_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 3 {
            attempt.error("too many redirects")
        } else if attempt.url().scheme() != "https" {
            attempt.error("redirect to a non-https target")
        } else {
            attempt.follow()
        }
    })
}

/// Performs the RFC 8058 POST. Returns the 2xx status; anything else — non-2xx
/// answer, blocked redirect, timeout, connect failure — is a 502 with a
/// caller-safe message. The response body is never read.
pub(crate) async fn post_one_click(
    client: &reqwest::Client,
    url: url::Url,
    timeout: Duration,
) -> Result<u16, ApiError> {
    let response = client
        .post(url)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(ONE_CLICK_BODY)
        .timeout(timeout)
        .send()
        .await
        .map_err(|error| {
            let reason = if error.is_timeout() {
                "the unsubscribe request timed out".to_string()
            } else if error.is_redirect() {
                "the list server redirected to a non-https target".to_string()
            } else {
                "could not reach the list server".to_string()
            };
            ApiError::new(StatusCode::BAD_GATEWAY, ApiErrorCode::NetworkError, reason)
        })?;
    let status = response.status();
    if status.is_success() {
        Ok(status.as_u16())
    } else {
        Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            ApiErrorCode::NetworkError,
            format!("the list server answered HTTP {}", status.as_u16()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use axum::routing::post;
    use axum::Router;

    use super::*;

    fn api_error_parts(error: ApiError) -> (StatusCode, ApiErrorCode, String) {
        (error.status, error.body.code, error.body.message)
    }

    // --- URL re-validation: rejected before any request is built ---

    #[test]
    fn validation_rejects_non_https_and_games() {
        for url in [
            "http://example.com/unsub",
            "ftp://example.com/unsub",
            "https://user:pass@example.com/unsub",
            "https://example.com@evil.com/unsub",
            "https://127.0.0.1/unsub",
            "https://[::1]/unsub",
            "not a url",
            "",
        ] {
            let error = validated_one_click_url(url).expect_err(url);
            let (status, code, _) = api_error_parts(error);
            assert_eq!(status, StatusCode::BAD_REQUEST, "url: {url}");
            assert_eq!(code, ApiErrorCode::UnsubscribeUnavailable, "url: {url}");
        }
    }

    #[test]
    fn validation_accepts_normal_https() {
        let url = validated_one_click_url("https://lists.example.com/unsub?u=1")
            .map_err(api_error_parts)
            .unwrap();
        assert_eq!(url.scheme(), "https");
    }

    // --- Transport behavior against a loopback mock ---
    //
    // The mock cannot serve TLS, so these use `build_one_click_client(false)`;
    // scheme enforcement is covered by the validation tests above and by
    // `https_only_client_never_reaches_a_plain_http_server` below.

    async fn serve(router: Router) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        tokio::spawn(async move {
            axum::serve(listener, router).await.expect("serve mock");
        });
        addr
    }

    #[tokio::test]
    async fn success_posts_the_rfc8058_body_and_surfaces_2xx() {
        let seen: Arc<std::sync::Mutex<Vec<(String, String)>>> = Arc::default();
        let seen_handler = Arc::clone(&seen);
        let addr = serve(Router::new().route(
            "/unsub",
            post(move |headers: axum::http::HeaderMap, body: String| {
                let seen = Arc::clone(&seen_handler);
                async move {
                    let content_type = headers
                        .get(header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    seen.lock().unwrap().push((content_type, body));
                    StatusCode::OK
                }
            }),
        ))
        .await;

        let url = url::Url::parse(&format!("http://{addr}/unsub")).unwrap();
        let status = post_one_click(&build_one_click_client(false), url, ONE_CLICK_TIMEOUT)
            .await
            .map_err(api_error_parts)
            .expect("2xx should succeed");
        assert_eq!(status, 200);
        let requests = seen.lock().unwrap();
        assert_eq!(
            requests.as_slice(),
            &[(
                "application/x-www-form-urlencoded".to_string(),
                ONE_CLICK_BODY.to_string()
            )]
        );
    }

    #[tokio::test]
    async fn non_2xx_is_surfaced_as_bad_gateway_without_body() {
        for mock_status in [StatusCode::BAD_REQUEST, StatusCode::INTERNAL_SERVER_ERROR] {
            let addr = serve(Router::new().route(
                "/unsub",
                post(move || async move { (mock_status, "secret upstream body") }),
            ))
            .await;
            let url = url::Url::parse(&format!("http://{addr}/unsub")).unwrap();
            let error = post_one_click(&build_one_click_client(false), url, ONE_CLICK_TIMEOUT)
                .await
                .expect_err("non-2xx should fail");
            let (status, code, message) = api_error_parts(error);
            assert_eq!(status, StatusCode::BAD_GATEWAY);
            assert_eq!(code, ApiErrorCode::NetworkError);
            assert!(
                message.contains(&mock_status.as_u16().to_string()),
                "message should carry the upstream status: {message}"
            );
            assert!(
                !message.contains("secret upstream body"),
                "upstream body must never be surfaced: {message}"
            );
        }
    }

    #[tokio::test]
    async fn redirect_downgrade_is_blocked() {
        let addr = serve(Router::new().route(
            "/unsub",
            post(|| async {
                (
                    StatusCode::FOUND,
                    [(header::LOCATION, "http://127.0.0.1:9/next")],
                )
            }),
        ))
        .await;
        let url = url::Url::parse(&format!("http://{addr}/unsub")).unwrap();
        let error = post_one_click(&build_one_click_client(false), url, ONE_CLICK_TIMEOUT)
            .await
            .expect_err("insecure redirect should fail");
        let (status, code, message) = api_error_parts(error);
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(code, ApiErrorCode::NetworkError);
        assert!(message.contains("redirect"), "message: {message}");
    }

    #[tokio::test]
    async fn slow_server_hits_the_deadline() {
        let addr = serve(Router::new().route(
            "/unsub",
            post(|| async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                StatusCode::OK
            }),
        ))
        .await;
        let url = url::Url::parse(&format!("http://{addr}/unsub")).unwrap();
        let error = post_one_click(
            &build_one_click_client(false),
            url,
            Duration::from_millis(100),
        )
        .await
        .expect_err("deadline should trip");
        let (status, code, message) = api_error_parts(error);
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(code, ApiErrorCode::NetworkError);
        assert!(message.contains("timed out"), "message: {message}");
    }

    #[tokio::test]
    async fn https_only_client_never_reaches_a_plain_http_server() {
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_handler = Arc::clone(&hits);
        let addr = serve(Router::new().route(
            "/unsub",
            post(move || {
                let hits = Arc::clone(&hits_handler);
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    StatusCode::OK
                }
            }),
        ))
        .await;
        // Defense in depth: even if validation were bypassed, the production
        // client (https_only) refuses the scheme before any connection.
        let url = url::Url::parse(&format!("http://{addr}/unsub")).unwrap();
        let error = post_one_click(&build_one_click_client(true), url, ONE_CLICK_TIMEOUT)
            .await
            .expect_err("https-only client must refuse http");
        let (status, code, _) = api_error_parts(error);
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(code, ApiErrorCode::NetworkError);
        assert_eq!(hits.load(Ordering::SeqCst), 0, "no request may be sent");
    }
}
