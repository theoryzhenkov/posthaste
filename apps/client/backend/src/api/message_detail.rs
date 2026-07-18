//! The message-detail family: one message opened for reading, and the raw
//! RFC 822 source.

use super::{offload_read, ApiFailure};
use crate::AppState;
use posthaste_client_models::{
    MessageDetailQuery, MessageDetailResult, MessageRawSourceQuery, MessageRawSourceResult,
};

pub(crate) async fn evaluate_message_detail(
    app: &AppState,
    query: MessageDetailQuery,
) -> Result<MessageDetailResult, ApiFailure> {
    // The id may be a stable draft key (the undo-restore reopen, D173):
    // resolve it to the live row the registry maps it to before reading.
    let message_id = app
        .service
        .resolve_live_message_id(&query.account_id, &query.message_id)?;
    // The gateway is optional: connected accounts fetch a missing body
    // lazily; offline the cached projection serves.
    let gateway = app.supervisor.gateway(&query.account_id).await.ok();
    let result = app
        .service
        .get_message_detail(&query.account_id, &message_id, gateway.as_deref())
        .await?;
    // A lazy body fetch is a committed write: publish its events so other
    // clients observe the cache fill.
    app.events.publish(&result.events);
    let detail = result
        .detail
        .ok_or_else(|| ApiFailure::unknown_id(format!("message {}", query.message_id.as_str())))?;
    Ok(MessageDetailResult {
        summary: detail.summary,
        body_html: detail.body_html,
        body_text: detail.body_text,
        attachments: detail.attachments,
        list_unsubscribe: detail.list_unsubscribe,
    })
}

pub(crate) async fn evaluate_message_raw_source(
    app: &AppState,
    query: MessageRawSourceQuery,
) -> Result<MessageRawSourceResult, ApiFailure> {
    if read_cached_raw(app, &query).await?.is_none() {
        // Not cached yet: the detail read's lazy body fetch also caches the
        // raw MIME when a gateway is reachable — the same incidental-effect
        // pattern as the messageDetail body fetch, events published alike.
        // (A missing message fails this read with unknown-id.)
        let gateway = app.supervisor.gateway(&query.account_id).await.ok();
        let result = app
            .service
            .get_message_detail(&query.account_id, &query.message_id, gateway.as_deref())
            .await?;
        if !result.events.is_empty() {
            app.events.publish(&result.events);
        }
    }
    let bytes = read_cached_raw(app, &query).await?.ok_or_else(|| {
        ApiFailure::unavailable("the message's raw source is not cached and cannot be fetched now")
    })?;
    Ok(MessageRawSourceResult {
        raw: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

async fn read_cached_raw(
    app: &AppState,
    query: &MessageRawSourceQuery,
) -> Result<Option<Vec<u8>>, ApiFailure> {
    let store = app.store.clone();
    let account_id = query.account_id.clone();
    let message_id = query.message_id.clone();
    offload_read(move || Ok(store.read_raw_message(&account_id, &message_id)?)).await
}
