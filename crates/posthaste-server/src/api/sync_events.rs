use super::*;

/// Request body for a manual source sync command.
///
/// @spec docs/L1-api#sync-and-events
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TriggerSyncRequest {
    #[serde(default)]
    pub mode: SyncMode,
}

/// Query parameters for the SSE event stream endpoint.
///
/// @spec docs/L1-api#sse-event-stream
#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct EventsQuery {
    pub account_id: Option<String>,
    pub topic: Option<String>,
    pub mailbox_id: Option<String>,
    pub after_seq: Option<i64>,
}

/// Response from `POST /v1/sources/{id}/commands/sync`.
///
/// @spec docs/L1-api#sync-and-events
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TriggerSyncResponse {
    pub ok: bool,
    pub event_count: usize,
    pub mode: String,
}

/// POST /v1/sources/{source_id}/commands/sync
///
/// @spec docs/L1-api#sync-and-events
/// @spec docs/L1-sync#sync-loop
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/commands/sync",
    tag = "sync",
    summary = "Trigger sync",
    description = "Runs a manual sync for a source and reports the number of events emitted.",
    params(("source_id" = String, Path, description = "Source (account) identifier")),
    // NOTE: the handler accepts an absent body (defaults to incremental sync). utoipa's
    // path macro can't emit `requestBody.required: false`, so optionality is documented here.
    request_body(content = TriggerSyncRequest,
        description = "Optional. Defaults to an incremental sync when the body is omitted."),
    responses(
        (status = 200, description = "Sync result", body = TriggerSyncResponse),
        (status = 404, description = "Source not found", body = ApiErrorBody),
        (status = 503, description = "Gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn trigger_sync(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    request: Option<Json<TriggerSyncRequest>>,
) -> Result<Json<TriggerSyncResponse>, ApiError> {
    let account_id = AccountId(source_id);
    let mode = request
        .map(|Json(request)| request.mode)
        .unwrap_or_default();
    let event_count = state
        .runtime
        .sync_account(RuntimeCaller::api(), account_id, mode)
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(TriggerSyncResponse {
        ok: true,
        event_count,
        mode: mode.as_str().to_string(),
    }))
}

/// GET /v1/events
///
/// Opens an SSE stream. When `afterSeq` is provided, replays matching events
/// from the backlog before switching to the live broadcast stream.
///
/// @spec docs/L1-api#sse-event-stream
/// @spec docs/L0-api#server-sent-events-for-push
// NOTE: utoipa cannot infer the SSE payload type. The full event payload contract
// (DomainEvent over text/event-stream) is documented in P3 via AsyncAPI.
#[utoipa::path(
    get,
    path = "/v1/events",
    tag = "events",
    summary = "Stream events",
    description = "Opens a Server-Sent Events stream of domain events. When afterSeq is provided, \
                   replays matching backlog events before switching to the live stream.",
    params(EventsQuery),
    responses(
        (status = 200, description = "Server-sent event stream of domain events", content_type = "text/event-stream"),
        (status = 400, description = "Invalid filter", body = ApiErrorBody)
    )
)]
pub async fn stream_events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let filter = EventFilter {
        account_id: query.account_id.map(AccountId),
        topic: query.topic,
        mailbox_id: query.mailbox_id.map(MailboxId),
        after_seq: query.after_seq,
    };
    let receiver = state.event_sender.subscribe();
    let backlog = if filter.after_seq.is_some() {
        state
            .service
            .list_events(&filter)
            .map_err(ApiError::from_service_error)?
    } else {
        Vec::new()
    };
    let replayed_through = backlog.last().map(|event| event.seq).or(filter.after_seq);
    let backlog_filter = filter.clone();
    let backlog_stream = tokio_stream::iter(
        backlog
            .into_iter()
            .filter(move |event| matches_event(event, &backlog_filter))
            .map(event_to_sse),
    );
    let live_filter = filter.clone();
    let live_stream = BroadcastStream::new(receiver).filter_map(move |message| {
        let live_filter = live_filter.clone();
        match message {
            Ok(event)
                if is_live_event_after_backlog(&event, replayed_through)
                    && matches_event(&event, &live_filter) =>
            {
                Some(event_to_sse(event))
            }
            _ => None,
        }
    });
    Ok(Sse::new(backlog_stream.chain(live_stream)).keep_alive(KeepAlive::default()))
}

pub(super) fn is_live_event_after_backlog(
    event: &DomainEvent,
    replayed_through: Option<i64>,
) -> bool {
    replayed_through.is_none_or(|seq| event.seq > seq)
}
