use super::*;

#[cfg(unix)]
#[test]
fn write_secure_file_creates_0600() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!(
        "posthaste-secure-file-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir should create");
    let path = dir.join("daemon.json");

    write_secure_file(&path, b"{\"port\":1}").expect("secure write should succeed");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "daemon.json must be owner-only");

    // Overwriting a pre-existing (here world-readable) file must re-tighten.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    write_secure_file(&path, b"{\"port\":2}").expect("secure overwrite should succeed");
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "overwrite must tighten back to 0600");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Build a temp frontend dist dir with an `index.html` (carrying a
/// `</head>`) and a static `app.js`, returning the dir for cleanup.
fn write_frontend_dist() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "posthaste-spa-fallback-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("temp dist dir should create");
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

    let _ = std::fs::remove_dir_all(&dir);
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

    let _ = std::fs::remove_dir_all(&dir);
}
