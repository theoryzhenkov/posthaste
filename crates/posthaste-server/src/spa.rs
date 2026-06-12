use super::*;

#[derive(Clone)]
struct SpaIndex {
    index_path: PathBuf,
    auth_token: String,
    port: u16,
}

/// Serve `index.html` with the auth token + port injected as globals so the
/// browser app can authenticate under `require_auth`. Mirrors the Tauri
/// `backend_init_script` injection: a `<script>` setting
/// `window.__POSTHASTE_TOKEN__` / `window.__POSTHASTE_PORT__` is spliced in
/// immediately before `</head>`. The web client reads the token and sends it
/// as `Authorization: Bearer` on every request — including the SSE stream and
/// logo/attachment fetches, which use `fetch()` rather than the native
/// `EventSource`/`<img>` so they can set the header. Static assets (JS/CSS) are
/// served by `ServeDir` and are unaffected.
///
/// @spec docs/eph/DESIGN-L1-trust-model
async fn serve_index_with_token(
    axum::extract::State(spa): axum::extract::State<SpaIndex>,
) -> Response {
    use axum::response::Html;

    let html = match tokio::fs::read_to_string(&spa.index_path).await {
        Ok(html) => html,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    // JSON-encode the token so it is safely quoted/escaped inside the JS string,
    // matching the Tauri init-script encoding. The port is a plain integer.
    let token_json =
        serde_json::to_string(&spa.auth_token).expect("auth token should serialize to JSON");
    let script = format!(
        "<script>window.__POSTHASTE_TOKEN__={token_json};window.__POSTHASTE_PORT__={};</script>",
        spa.port
    );

    // Splice the script in just before the (first) </head>, matched
    // case-insensitively so `</HEAD>`/`</Head>` also work; the original closing
    // tag is preserved verbatim. If no </head> exists (unexpected for a built
    // index.html) fall back to prepending the script.
    let injected = match html.to_ascii_lowercase().find("</head>") {
        Some(idx) => {
            let (head, rest) = html.split_at(idx);
            format!("{head}{script}{rest}")
        }
        None => format!("{script}{html}"),
    };

    Html(injected).into_response()
}

/// Build the browser-serve SPA fallback service: a `ServeDir` over the built
/// frontend whose own not-found falls through to [`serve_index_with_token`].
///
/// `append_index_html_on_directories(false)` is load-bearing: without it,
/// `ServeDir` auto-serves the raw `index.html` for `GET /` (bypassing token
/// injection, so the browser app would 401 under `require_auth`). With it off,
/// `/` and any SPA route miss in `ServeDir` and fall through to the injecting
/// handler; real asset files (e.g. `/app.js`) are still served verbatim.
///
/// @spec docs/eph/DESIGN-L1-trust-model
pub(crate) fn spa_fallback_service(
    frontend_dist: &std::path::Path,
    auth_token: &str,
    port: u16,
) -> ServeDir<axum::routing::MethodRouter> {
    let spa = SpaIndex {
        index_path: frontend_dist.join("index.html"),
        auth_token: auth_token.to_string(),
        port,
    };
    let index_service = get(serve_index_with_token).with_state(spa);
    ServeDir::new(frontend_dist)
        .append_index_html_on_directories(false)
        .fallback(index_service)
}
