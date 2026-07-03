use super::*;

use axum::extract::Request;
use axum::middleware::Next;
use tower_http::timeout::TimeoutLayer;

use crate::deadlines::{request_timeout_error, REQUEST_TIMEOUT};

async fn api_not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

/// The internal sentinel status [`TimeoutLayer`] emits when [`REQUEST_TIMEOUT`]
/// elapses. `tower_http::timeout::TimeoutLayer` (≥0.6.7) never produces a
/// `BoxError` — it returns a bare empty-body response at this status directly,
/// so no `HandleErrorLayer` is needed. `GATEWAY_TIMEOUT` is not otherwise
/// returned by any regular-route handler, so [`rewrite_timeout_response`] can
/// recognize it unambiguously and is never applied outside `build_api_router`.
const TIMEOUT_SENTINEL_STATUS: StatusCode = StatusCode::GATEWAY_TIMEOUT;

/// Rewrite the bare [`TIMEOUT_SENTINEL_STATUS`] response from [`TimeoutLayer`]
/// into the same typed `ApiErrorBody` JSON envelope every other `/v1` error
/// uses (D64; a timeout is Transient — M29 vocabulary — mapped onto the
/// existing `GatewayUnavailable` code, not a new one).
async fn rewrite_timeout_response(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    if response.status() == TIMEOUT_SENTINEL_STATUS {
        request_timeout_error("request").into_response()
    } else {
        response
    }
}

/// Apply the blanket per-request deadline (D64) to `router`. Factored out of
/// [`build_api_router`] so the M24 gate test below can exercise the identical
/// layering against a stub handler that never resolves, without duplicating
/// the wiring or standing up a real `AppState`/`RuntimeHandle`.
pub(crate) fn with_request_timeout<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .layer(TimeoutLayer::with_status_code(
            TIMEOUT_SENTINEL_STATUS,
            REQUEST_TIMEOUT,
        ))
        .layer(middleware::from_fn(rewrite_timeout_response))
}

/// Build the `/v1` API router: every handler route, the not-found fallback, and
/// the `require_auth` middleware, finished with `state`. Shared by
/// [`start_server`] and the integration tests so tests drive the REAL handlers
/// through the REAL auth perimeter (not stubs). The runtime-only outer layers
/// (request tracing, CORS) are applied by [`start_server`] on top of this, which
/// preserves the original layer order (cors → trace → auth → routes).
///
/// D64/M24: the SSE/stream routes (`/runtime/sessions/{id}/stream`, `/events`)
/// are split into their own sub-router so the blanket [`TimeoutLayer`] below —
/// applied only to `regular_routes` — cannot wrongly cut a long-lived stream. A
/// stream's own SETUP await is instead deadline-wrapped at the handler
/// (`crate::deadlines::with_stream_setup_deadline`); the streaming phase itself
/// stays unbounded here (keepalive/idle-reap is D68's unit).
pub fn build_api_router(state: Arc<AppState>) -> Router {
    let stream_routes = Router::new()
        .route(
            "/runtime/sessions/{session_id}/stream",
            get(api::runtime_stream::stream_runtime_link),
        )
        .route("/events", get(api::stream_events));

    let regular_routes = Router::new()
        .route("/health", get(api::health))
        .route(
            "/settings",
            get(api::get_settings).patch(api::patch_settings),
        )
        .route(
            "/automation-rules:preview",
            post(api::preview_automation_rule),
        )
        .route(
            "/accounts",
            get(api::list_accounts).post(api::create_account),
        )
        .route(
            "/accounts/{account_id}",
            get(api::get_account)
                .patch(api::patch_account)
                .delete(api::delete_account),
        )
        .route("/accounts/{account_id}/verify", post(api::verify_account))
        .route("/auth/tokens", post(api::create_auth_token))
        .route("/accounts/{account_id}/enable", post(api::enable_account))
        .route("/accounts/{account_id}/disable", post(api::disable_account))
        .route(
            "/accounts/{account_id}/logo",
            post(api::upload_account_logo),
        )
        .route(
            "/account-assets/logos/{image_id}",
            get(api::get_account_logo),
        )
        .route("/read", post(api::read))
        .route(
            "/smart-mailboxes",
            get(api::list_smart_mailboxes).post(api::create_smart_mailbox),
        )
        .route(
            "/smart-mailboxes/{smart_mailbox_id}",
            get(api::get_smart_mailbox)
                .patch(api::patch_smart_mailbox)
                .delete(api::delete_smart_mailbox),
        )
        .route(
            "/smart-mailboxes:reset-defaults",
            post(api::reset_default_smart_mailboxes),
        )
        .route(
            "/smart-mailboxes/{smart_mailbox_id}/messages",
            get(api::list_smart_mailbox_messages),
        )
        .route(
            "/smart-mailboxes/{smart_mailbox_id}/conversations",
            get(api::list_smart_mailbox_conversations),
        )
        .route("/views/conversations", get(api::list_conversations))
        .route(
            "/views/conversations/{conversation_id}",
            get(api::get_conversation),
        )
        .route("/sources/{source_id}/mailboxes", get(api::list_mailboxes))
        .route(
            "/sources/{source_id}/mailboxes/{mailbox_id}",
            patch(api::patch_mailbox),
        )
        .route(
            "/sources/{source_id}/messages",
            get(api::list_source_messages),
        )
        .route("/messages/search", get(api::search_messages))
        .route(
            "/runtime/sessions",
            post(api::runtime_stream::open_runtime_link),
        )
        .route(
            "/runtime/sessions/{session_id}",
            axum::routing::delete(api::runtime_stream::close_runtime_link),
        )
        .route(
            "/runtime/sessions/{session_id}/views",
            post(api::runtime_stream::open_runtime_link_view),
        )
        .route(
            "/runtime/sessions/{session_id}/views/{view_id}",
            axum::routing::delete(api::runtime_stream::close_runtime_link_view),
        )
        .route(
            "/runtime/sessions/{session_id}/views/{view_id}/extend",
            post(api::runtime_stream::extend_runtime_link_view),
        )
        .route(
            "/runtime/sessions/{session_id}/mutations",
            post(api::runtime_stream::run_runtime_link_mutation),
        )
        .route(
            "/runtime/sessions/{session_id}/mutations/{client_mutation_id}",
            get(api::runtime_stream::runtime_link_mutation_settlement),
        )
        .route(
            "/sources/{source_id}/messages/{message_id}",
            get(api::get_message),
        )
        .route(
            "/sources/{source_id}/messages/{message_id}/attachments/{attachment_id}",
            get(api::get_message_attachment),
        )
        .route(
            "/sources/{source_id}/messages/{message_id}/body",
            get(api::get_message_body),
        )
        .route("/sender-addresses", get(api::list_sender_addresses))
        .route("/sources/{source_id}/identity", get(api::get_identity))
        .route(
            "/sources/{source_id}/messages/{message_id}/reply-context",
            get(api::get_reply_context),
        )
        .route(
            "/sources/{source_id}/messages/{message_id}/draft-content",
            get(api::get_draft_content),
        )
        .route(
            "/sources/{source_id}/commands/send",
            post(api::send_message).layer(DefaultBodyLimit::max(SEND_MESSAGE_BODY_LIMIT_BYTES)),
        )
        .route(
            "/sources/{source_id}/commands/save-draft",
            post(api::save_draft).layer(DefaultBodyLimit::max(SEND_MESSAGE_BODY_LIMIT_BYTES)),
        )
        .route(
            "/sources/{source_id}/commands/delete-draft",
            post(api::delete_draft),
        )
        .route(
            "/sources/{source_id}/operations",
            get(api::list_pending_operations),
        )
        .route(
            "/sources/{source_id}/operations/{operation_id}",
            axum::routing::delete(api::discard_operation),
        )
        .route(
            "/sources/{source_id}/operations/{operation_id}/retry",
            post(api::retry_operation),
        )
        .route(
            "/sources/{source_id}/commands/messages/{message_id}/set-keywords",
            post(api::set_keywords),
        )
        .route(
            "/sources/{source_id}/commands/messages/{message_id}/add-to-mailbox",
            post(api::add_to_mailbox),
        )
        .route(
            "/sources/{source_id}/commands/messages/{message_id}/remove-from-mailbox",
            post(api::remove_from_mailbox),
        )
        .route(
            "/sources/{source_id}/commands/messages/{message_id}/replace-mailboxes",
            post(api::replace_mailboxes),
        )
        .route(
            "/sources/{source_id}/commands/messages/{message_id}/destroy",
            post(api::destroy_message),
        )
        .route("/config:reload", post(api::reload_config));
    let regular_routes = with_request_timeout(regular_routes);
    // Manual sync awaits the whole provider cycle end-to-end — its own, longer
    // bound (SYNC_REQUEST_TIMEOUT); same sentinel-rewrite envelope as the
    // blanket. Enqueue-and-return semantics are provider-M36 territory.
    let sync_routes = Router::new()
        .route(
            "/sources/{source_id}/commands/sync",
            post(api::trigger_sync),
        )
        .layer(axum::middleware::from_fn(rewrite_timeout_response))
        .layer(TimeoutLayer::with_status_code(
            TIMEOUT_SENTINEL_STATUS,
            crate::deadlines::SYNC_REQUEST_TIMEOUT,
        ));

    regular_routes
        .merge(sync_routes)
        .merge(stream_routes)
        .fallback(api_not_found)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth_layer,
        ))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::Body;
    use tower::ServiceExt;

    async fn hang_forever() -> StatusCode {
        std::future::pending::<()>().await;
        StatusCode::OK
    }

    /// M24 gate: a wedged runtime call inside a regular route must return a
    /// timeout response, not hang the handler forever (audit N10). This wires
    /// the exact same [`with_request_timeout`] layering `build_api_router`
    /// uses, over a stub handler that never resolves — no `AppState`/
    /// `RuntimeHandle` needed to prove the layer itself is load-bearing.
    #[tokio::test(start_paused = true)]
    async fn a_wedged_regular_route_times_out_instead_of_hanging() {
        let app = with_request_timeout(Router::new().route("/hang", get(hang_forever)));

        let request = axum::http::Request::builder()
            .uri("/hang")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.expect("service must not error");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bytes = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        let body: serde_json::Value = serde_json::from_slice(&bytes)
            .expect("timeout body must be the standard JSON error envelope");
        assert_eq!(body["code"], "gateway_unavailable");
    }
}
