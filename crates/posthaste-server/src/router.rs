use super::*;

async fn api_not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}

/// Build the `/v1` API router: every handler route, the not-found fallback, and
/// the `require_auth` middleware, finished with `state`. Shared by
/// [`start_server`] and the integration tests so tests drive the REAL handlers
/// through the REAL auth perimeter (not stubs). The runtime-only outer layers
/// (request tracing, CORS) are applied by [`start_server`] on top of this, which
/// preserves the original layer order (cors → trace → auth → routes).
pub fn build_api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(api::health))
        .route("/openapi.json", get(openapi::openapi_json))
        .route("/asyncapi.json", get(openapi::asyncapi_json))
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
        .route(
            "/accounts/{account_id}/oauth/start",
            post(api::start_account_oauth),
        )
        .route("/oauth/start", post(api::start_provider_oauth))
        .route("/oauth/callback", get(api::complete_account_oauth))
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
            post(api::runtime_stream::open_runtime_session),
        )
        .route(
            "/runtime/sessions/{session_id}",
            axum::routing::delete(api::runtime_stream::close_runtime_session),
        )
        .route(
            "/runtime/sessions/{session_id}/stream",
            get(api::runtime_stream::stream_runtime_session),
        )
        .route(
            "/runtime/sessions/{session_id}/views",
            post(api::runtime_stream::open_runtime_session_view),
        )
        .route(
            "/runtime/sessions/{session_id}/views/{view_id}",
            axum::routing::delete(api::runtime_stream::close_runtime_session_view),
        )
        .route(
            "/runtime/sessions/{session_id}/views/{view_id}/extend",
            post(api::runtime_stream::extend_runtime_session_view),
        )
        .route(
            "/runtime/sessions/{session_id}/mutations",
            post(api::runtime_stream::run_runtime_session_mutation),
        )
        .route("/views", post(api::open_view))
        .route("/views/{view_id}/stream", get(api::stream_view))
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
        .route(
            "/sources/{source_id}/commands/sync",
            post(api::trigger_sync),
        )
        .route("/config:reload", post(api::reload_config))
        .route("/events", get(api::stream_events))
        .fallback(api_not_found)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth_layer,
        ))
        .with_state(state)
}
