use super::*;
use posthaste_runtime_contract::{ViewDescriptor, ViewFrame, ViewId, ViewRevision, ViewSnapshot};

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenViewRequest {
    #[schema(value_type = Object)]
    pub descriptor: ViewDescriptor,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenViewResponse {
    #[schema(value_type = String)]
    pub view_id: ViewId,
    #[schema(value_type = Object)]
    pub snapshot: ViewSnapshot,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct OpenViewQuery {
    pub source_id: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ViewStreamQuery {
    pub after_revision: Option<u64>,
    pub source_id: Option<String>,
}

#[utoipa::path(
    post,
    path = "/v1/views",
    tag = "views",
    summary = "Open a runtime view",
    description = "Opens a runtime-owned view and returns its initial snapshot.",
    request_body = OpenViewRequest,
    params(OpenViewQuery),
    responses(
        (status = 200, description = "The opened view snapshot", body = OpenViewResponse),
        (status = 400, description = "Invalid view descriptor", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn open_view(
    State(state): State<Arc<AppState>>,
    Query(query): Query<OpenViewQuery>,
    Json(request): Json<OpenViewRequest>,
) -> Result<Json<OpenViewResponse>, ApiError> {
    let snapshot = state
        .runtime
        .open_view(view_caller(query.source_id.as_deref()), request.descriptor)
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OpenViewResponse {
        view_id: snapshot.view_id.clone(),
        snapshot,
    }))
}

#[utoipa::path(
    get,
    path = "/v1/views/{view_id}/stream",
    tag = "views",
    summary = "Subscribe to a runtime view stream",
    description = "Streams runtime view frames as server-sent events. Event ids are view revisions.",
    params(
        ("view_id" = String, Path, description = "Runtime view id"),
        ViewStreamQuery
    ),
    responses(
        (status = 200, description = "SSE stream of runtime ViewFrame values"),
        (status = 404, description = "Unknown view", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn stream_view(
    State(state): State<Arc<AppState>>,
    Path(view_id): Path<String>,
    Query(query): Query<ViewStreamQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let subscription = state
        .runtime
        .subscribe_view(
            view_caller(query.source_id.as_deref()),
            ViewId::new(view_id),
            query.after_revision.map(ViewRevision::new),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    let catch_up_stream =
        tokio_stream::iter(subscription.catch_up.into_iter().map(view_frame_to_sse));
    let live_stream = subscription.live.map(view_frame_to_sse);
    Ok(Sse::new(catch_up_stream.chain(live_stream)).keep_alive(KeepAlive::default()))
}

fn view_caller(source_id: Option<&str>) -> RuntimeCaller {
    let mut caller = RuntimeCaller::api();
    caller.account_scope = source_id.map(|source_id| vec![source_id.to_string()]);
    caller
}

fn view_frame_to_sse(frame: ViewFrame) -> Result<Event, Infallible> {
    let event = frame
        .revision()
        .map(|revision| Event::default().id(revision.get().to_string()))
        .unwrap_or_default();
    Ok(event
        .json_data(frame)
        .unwrap_or_else(|_| Event::default().data("{}")))
}
