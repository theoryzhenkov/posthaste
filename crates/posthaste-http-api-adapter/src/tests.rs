use super::*;
use crate::test_support::{temp_root, TempDirGuard};

#[cfg(unix)]
#[test]
fn write_secure_file_creates_0600() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_root("secure-file-test");
    let path = dir.join("daemon.json");

    write_secure_file(&path, b"{\"port\":1}").expect("secure write should succeed");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "daemon.json must be owner-only");

    // Overwriting a pre-existing (here world-readable) file must re-tighten.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    write_secure_file(&path, b"{\"port\":2}").expect("secure overwrite should succeed");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "overwrite must tighten back to 0600");
}

/// Build a temp frontend dist dir with an `index.html` (carrying a
/// `</head>`) and a static `app.js`, returning the guard (keep it bound —
/// the directory is removed when it drops).
fn write_frontend_dist() -> TempDirGuard {
    let dir = temp_root("spa-fallback-test");
    std::fs::write(
        dir.join("index.html"),
        "<!doctype html><html><head><title>Posthaste</title></head><body>app</body></html>",
    )
    .expect("index.html should write");
    std::fs::write(dir.join("app.js"), "console.log('app');\n").expect("app.js should write");
    dir
}

async fn body_string(response: axum::response::Response) -> String {
    use http_body_util::BodyExt;
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body should collect")
        .to_bytes();
    String::from_utf8(bytes.to_vec()).expect("body should be utf-8")
}

/// `GET /` must fall through to the injecting handler (NOT ServeDir's raw
/// index auto-serve), returning `text/html` with the token script spliced
/// in before `</head>`. This guards the `append_index_html_on_directories(false)`
/// wiring: without it, `/` served the raw index with no token.
#[tokio::test]
async fn root_serves_index_with_injected_token() {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    let dir = write_frontend_dist();
    let app = Router::new().fallback_service(spa_fallback_service(&dir, "the-correct-token", 4321));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        content_type.starts_with("text/html"),
        "index must be served as text/html, got {content_type:?}"
    );

    let body = body_string(response).await;
    assert!(
        body.contains("window.__POSTHASTE_TOKEN__=\"the-correct-token\""),
        "served index must carry the injected token, got: {body}"
    );
    assert!(
        body.contains("window.__POSTHASTE_PORT__=4321"),
        "served index must carry the injected port, got: {body}"
    );
    // The script is spliced before the original </head>, which is preserved.
    let script_idx = body
        .find("__POSTHASTE_TOKEN__")
        .expect("token script present");
    let head_idx = body.find("</head>").expect("</head> preserved");
    assert!(script_idx < head_idx, "token script must precede </head>");
}

/// A real static asset is served verbatim by ServeDir, with no token
/// injection (only the SPA fallback document gets the script).
#[tokio::test]
async fn static_asset_served_verbatim() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let dir = write_frontend_dist();
    let app = Router::new().fallback_service(spa_fallback_service(&dir, "the-correct-token", 4321));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/app.js")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router should respond");

    assert_eq!(response.status(), StatusCode::OK);
    let body = body_string(response).await;
    assert_eq!(
        body, "console.log('app');\n",
        "static asset must be served verbatim, with no injection"
    );
    assert!(
        !body.contains("__POSTHASTE_TOKEN__"),
        "static assets must never carry the injected token"
    );
}

// ---- M30: /v1 boundary sanitation, operator logging, deny reasons, fail_closed ----

use std::sync::{Arc, Mutex};

/// A `MakeWriter` capturing all formatted log output into a shared buffer, so a
/// test can assert on what crossed the operator log without a real subscriber.
#[derive(Clone)]
struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureWriter {
    type Writer = CaptureWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Run `f` with a thread-local subscriber that captures every log line, and
/// return `(f's value, captured logs)`.
fn capture_logs<R>(f: impl FnOnce() -> R) -> (R, String) {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_writer(CaptureWriter(buf.clone()))
        .with_max_level(tracing::Level::TRACE)
        .without_time()
        .finish();
    let value = tracing::subscriber::with_default(subscriber, f);
    let logs = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    (value, logs)
}

/// D72: a 5xx runtime error must be sanitized at the boundary — the client body
/// carries a generic message + a correlation id, never the io/sql cause, while
/// the real cause AND that same correlation id reach the operator log so the two
/// can be joined.
#[tokio::test]
async fn m30_internal_error_body_is_sanitized_and_cause_is_logged() {
    use posthaste_contract_core::{RuntimeError, RuntimeErrorCode};

    let cause = "sqlite: disk I/O error opening /var/db/posthaste/mail.sqlite";
    let (response, logs) = capture_logs(|| {
        crate::api::ApiError::from_runtime_error(RuntimeError::new(
            RuntimeErrorCode::StorageFailure,
            cause,
        ))
        .into_response()
    });

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = body_string(response).await;

    // The wire body leaks no server-internal detail.
    assert!(
        !body.contains("sqlite") && !body.contains("disk I/O") && !body.contains("/var/db"),
        "5xx body must not carry io/sql cause text: {body}"
    );
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("body is JSON");
    assert_eq!(
        parsed["message"], "internal error",
        "generic message on the wire"
    );
    let correlation_id = parsed["details"]["correlationId"]
        .as_str()
        .expect("body carries a correlation id")
        .to_string();
    assert!(!correlation_id.is_empty());

    // The operator log carries the real cause AND the correlation id that is on
    // the wire — the join key between a 500 body and its cause.
    assert!(
        logs.contains(cause),
        "cause must reach the server log: {logs}"
    );
    assert!(
        logs.contains("http.internal_error"),
        "5xx construction must emit the typed error event: {logs}"
    );
    assert!(
        logs.contains(&correlation_id),
        "log must carry the same correlation id as the body: {logs}"
    );
}

/// M2 safety gate at the wire: the `MailboxNotEmpty` refusal the service raises
/// for a `DELETE …/mailboxes/{id}` without `removeEmails=true` maps to a 409
/// Conflict whose body carries the machine code `mailbox_not_empty` and the
/// message count in `details.count`, so the client can drive the
/// confirm-with-count dialog. The refusal never reaches the provider (proved in
/// the service-layer gate test); this asserts the shape the client sees.
#[tokio::test]
async fn delete_non_empty_mailbox_without_remove_emails_maps_to_409_with_count() {
    use posthaste_contract_core::RuntimeError;
    use posthaste_domain_model::{GatewayError, ServiceError};

    // The exact error `MailService::destroy_mailbox` returns when the gate refuses.
    let runtime_error = RuntimeError::from(ServiceError::Gateway(GatewayError::MailboxNotEmpty {
        count: 7,
    }));
    let response = crate::api::ApiError::from_runtime_error(runtime_error).into_response();

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = body_string(response).await;
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("body is JSON");
    assert_eq!(
        parsed["code"], "mailbox_not_empty",
        "distinct code lets the client detect the confirm-with-count case: {body}"
    );
    assert_eq!(
        parsed["details"]["count"], 7,
        "the 409 carries the message count for the dialog: {body}"
    );
}

/// D72: a 4xx runtime error is caller-actionable — its message is NOT sanitized
/// away (this guards against over-redaction of the useful validation text).
#[tokio::test]
async fn m30_client_error_message_is_preserved() {
    use posthaste_contract_core::RuntimeError;

    let (response, _logs) = capture_logs(|| {
        crate::api::ApiError::from_runtime_error(RuntimeError::invalid_mutation(
            "request link id does not match path link id",
        ))
        .into_response()
    });
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = body_string(response).await;
    assert!(
        body.contains("request link id does not match path link id"),
        "actionable 4xx message must survive sanitization: {body}"
    );
}

/// D72: an authz caveat denial logs its (non-sensitive) reason instead of
/// discarding it.
#[test]
fn m30_authz_deny_reason_is_logged() {
    let root = crate::token::RootKey::from_test_bytes([7u8; 32]);
    // Token scoped to account `acct-a`; request targets `acct-b` → out of scope.
    let token = crate::token::mint_with_caveats(&root, &["account = acct-a"]);
    let presented = crate::auth::PresentedToken(token);
    let ctx = crate::authz::CaveatContext {
        action: crate::authz::Action::Read,
        account: Some("acct-b".to_string()),
        mailbox: None,
        message: None,
        now: time::OffsetDateTime::now_utc(),
    };

    let (result, logs) = capture_logs(|| {
        crate::auth::authorize_presented_caveats(Some(&presented), &root, &ctx, "test route")
    });

    assert!(result.is_err(), "an out-of-scope token must be denied");
    assert!(
        logs.contains("http.authz.denied"),
        "deny must emit the typed authz event: {logs}"
    );
    assert!(
        logs.contains("account out of scope"),
        "the deny reason must be logged, not discarded: {logs}"
    );
}

/// D73: `fail_closed!` logs its reason at `error!` BEFORE it panics, so a
/// deliberate fail-closed abort is diagnosable in the operator log.
#[test]
fn m30_fail_closed_logs_before_panicking() {
    use posthaste_observability::fail_closed;

    // Silence the default panic hook so the intentional panic does not spam
    // stderr; restore it afterward.
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let (outcome, logs) = capture_logs(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            fail_closed!("tls acceptor missing key: {}", "reason-42")
        }))
    });
    std::panic::set_hook(previous_hook);

    assert!(outcome.is_err(), "fail_closed! must panic");
    assert!(
        logs.contains("fail_closed"),
        "the fail-closed event must be logged: {logs}"
    );
    assert!(
        logs.contains("tls acceptor missing key: reason-42"),
        "the fail-closed reason must be logged before the panic: {logs}"
    );
}
