use super::*;
use posthaste_runtime_contract::{
    MutationReceipt, MutationRequest, RuntimeError, RuntimeFrame, RuntimeSession, RuntimeSessionId,
    RuntimeSessionSeq, ViewDescriptor, ViewId, ViewSnapshot,
};

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionQuery {
    pub source_id: Option<String>,
    /// The session can apply incremental mail-list deltas
    /// ([replication client-link L1](../../../docs/replication/client-link/L1.md)); when `true` the
    /// runtime sends `ViewDelta` frames instead of whole `ViewReplace`s.
    #[serde(default)]
    pub view_delta: Option<bool>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSessionStreamQuery {
    pub after_seq: Option<u64>,
    pub source_id: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenRuntimeSessionViewRequest {
    #[schema(value_type = Object)]
    pub descriptor: ViewDescriptor,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenRuntimeSessionViewResponse {
    #[schema(value_type = String)]
    pub view_id: ViewId,
    pub snapshot: ViewSnapshot,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExtendRuntimeSessionViewRequest {
    /// Number of additional rows to grow the view's window by.
    pub count: usize,
}

#[utoipa::path(
    post,
    path = "/v1/runtime/sessions",
    tag = "runtime",
    summary = "Open a runtime session",
    description = "Creates a runtime session whose stream carries RuntimeFrame values.",
    params(RuntimeSessionQuery),
    responses(
        (status = 200, description = "The opened runtime session", body = RuntimeSession),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn open_runtime_session(
    State(state): State<Arc<AppState>>,
    Query(query): Query<RuntimeSessionQuery>,
) -> Result<Json<RuntimeSession>, ApiError> {
    let mut caller = runtime_caller(query.source_id.as_deref());
    caller.capabilities.view_delta = query.view_delta.unwrap_or(false);
    state
        .runtime
        .open_session(caller)
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

#[utoipa::path(
    delete,
    path = "/v1/runtime/sessions/{session_id}",
    tag = "runtime",
    summary = "Close a runtime session",
    description = "Closes a runtime session and releases its open runtime views.",
    params(
        ("session_id" = String, Path, description = "Runtime session id"),
        RuntimeSessionQuery
    ),
    responses(
        (status = 200, description = "The session was closed", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime session", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn close_runtime_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Query(query): Query<RuntimeSessionQuery>,
) -> Result<Json<OkResponse>, ApiError> {
    state
        .runtime
        .close_session(
            runtime_caller(query.source_id.as_deref()),
            RuntimeSessionId::new(session_id),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OkResponse { ok: true }))
}

#[utoipa::path(
    get,
    path = "/v1/runtime/sessions/{session_id}/stream",
    tag = "runtime",
    summary = "Subscribe to runtime frames",
    description = "Streams session-scoped RuntimeFrame values as server-sent events. Event ids are sessionSeq values.",
    params(
        ("session_id" = String, Path, description = "Runtime session id"),
        RuntimeSessionStreamQuery
    ),
    responses(
        (status = 200, description = "SSE stream of RuntimeFrame values"),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime session", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn stream_runtime_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Query(query): Query<RuntimeSessionStreamQuery>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let subscription = state
        .runtime
        .subscribe_runtime_frames(
            runtime_caller(query.source_id.as_deref()),
            RuntimeSessionId::new(session_id),
            query.after_seq.map(RuntimeSessionSeq::new),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    let catch_up_stream = tokio_stream::iter(subscription.catch_up.into_iter().map(frame_to_sse));
    let live_stream = subscription.live.map(frame_to_sse);
    Ok(Sse::new(catch_up_stream.chain(live_stream)).keep_alive(KeepAlive::default()))
}

#[utoipa::path(
    post,
    path = "/v1/runtime/sessions/{session_id}/views",
    tag = "runtime",
    summary = "Open a runtime session view",
    description = "Opens a view in a runtime session and returns its initial snapshot.",
    params(
        ("session_id" = String, Path, description = "Runtime session id"),
        RuntimeSessionQuery
    ),
    request_body = OpenRuntimeSessionViewRequest,
    responses(
        (status = 200, description = "The opened view snapshot", body = OpenRuntimeSessionViewResponse),
        (status = 400, description = "Invalid view descriptor", body = ApiErrorBody),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime session", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn open_runtime_session_view(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Query(query): Query<RuntimeSessionQuery>,
    Json(request): Json<OpenRuntimeSessionViewRequest>,
) -> Result<Json<OpenRuntimeSessionViewResponse>, ApiError> {
    let snapshot = state
        .runtime
        .open_session_view(
            runtime_caller(query.source_id.as_deref()),
            RuntimeSessionId::new(session_id),
            request.descriptor,
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OpenRuntimeSessionViewResponse {
        view_id: snapshot.view_id.clone(),
        snapshot,
    }))
}

#[utoipa::path(
    delete,
    path = "/v1/runtime/sessions/{session_id}/views/{view_id}",
    tag = "runtime",
    summary = "Close a runtime session view",
    description = "Closes a runtime view for a session and emits a viewClosed RuntimeFrame.",
    params(
        ("session_id" = String, Path, description = "Runtime session id"),
        ("view_id" = String, Path, description = "Runtime view id"),
        RuntimeSessionQuery
    ),
    responses(
        (status = 200, description = "The view was closed", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime session", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn close_runtime_session_view(
    State(state): State<Arc<AppState>>,
    Path((session_id, view_id)): Path<(String, String)>,
    Query(query): Query<RuntimeSessionQuery>,
) -> Result<Json<OkResponse>, ApiError> {
    state
        .runtime
        .close_session_view(
            runtime_caller(query.source_id.as_deref()),
            RuntimeSessionId::new(session_id),
            ViewId::new(view_id),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OkResponse { ok: true }))
}

#[utoipa::path(
    post,
    path = "/v1/runtime/sessions/{session_id}/views/{view_id}/extend",
    tag = "runtime",
    summary = "Extend a runtime session view window",
    description = "Grows an open windowed view (e.g. mailList) by the requested row count and returns the extended snapshot, also broadcast as a viewReplace RuntimeFrame.",
    params(
        ("session_id" = String, Path, description = "Runtime session id"),
        ("view_id" = String, Path, description = "Runtime view id"),
        RuntimeSessionQuery
    ),
    request_body = ExtendRuntimeSessionViewRequest,
    responses(
        (status = 200, description = "The extended view snapshot", body = OpenRuntimeSessionViewResponse),
        (status = 400, description = "View does not support window extension", body = ApiErrorBody),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime session or view", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn extend_runtime_session_view(
    State(state): State<Arc<AppState>>,
    Path((session_id, view_id)): Path<(String, String)>,
    Query(query): Query<RuntimeSessionQuery>,
    Json(request): Json<ExtendRuntimeSessionViewRequest>,
) -> Result<Json<OpenRuntimeSessionViewResponse>, ApiError> {
    let snapshot = state
        .runtime
        .extend_session_view(
            runtime_caller(query.source_id.as_deref()),
            RuntimeSessionId::new(session_id),
            ViewId::new(view_id),
            request.count,
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    Ok(Json(OpenRuntimeSessionViewResponse {
        view_id: snapshot.view_id.clone(),
        snapshot,
    }))
}

#[utoipa::path(
    post,
    path = "/v1/runtime/sessions/{session_id}/mutations",
    tag = "runtime",
    summary = "Run a runtime mutation",
    description = "Submits a named mutation to a runtime session (message read/flag/tags/move/archive/trash/restore/destroy) and emits mutationSettlement RuntimeFrame values on the session stream.",
    params(
        ("session_id" = String, Path, description = "Runtime session id"),
        RuntimeSessionQuery
    ),
    request_body = MutationRequest,
    responses(
        (status = 200, description = "Mutation receipt", body = MutationReceipt),
        (status = 400, description = "Invalid mutation", body = ApiErrorBody),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 403, description = "Forbidden", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime session", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn run_runtime_session_mutation(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Query(query): Query<RuntimeSessionQuery>,
    presented: Option<Extension<crate::auth::PresentedToken>>,
    Json(mut request): Json<MutationRequest>,
) -> Result<Json<MutationReceipt>, ApiError> {
    let path_session_id = RuntimeSessionId::new(session_id);
    if request
        .session_id
        .as_ref()
        .is_some_and(|body_session_id| body_session_id != &path_session_id)
    {
        return Err(ApiError::from_runtime_error(
            RuntimeError::invalid_mutation("request session id does not match path session id"),
        ));
    }
    request.session_id = Some(path_session_id);
    require_read_for_session_mutation(
        state.as_ref(),
        query.source_id.as_deref(),
        presented.as_ref().map(|Extension(token)| token),
    )?;
    state
        .runtime
        .run_mutation(runtime_caller(query.source_id.as_deref()), request)
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

fn require_read_for_session_mutation(
    state: &AppState,
    source_id: Option<&str>,
    presented: Option<&crate::auth::PresentedToken>,
) -> Result<(), ApiError> {
    if !state.require_auth {
        return Ok(());
    }
    let Some(presented) = presented else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            ApiErrorCode::Unauthorized,
            "missing or invalid bearer token",
        ));
    };
    let caveats = crate::token::verify_authenticity(&presented.0, &state.macaroon_root_key)
        .map_err(|_| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                ApiErrorCode::Unauthorized,
                "missing or invalid bearer token",
            )
        })?;
    if caveats.is_empty() {
        return Ok(());
    }
    let ctx = crate::authz::CaveatContext {
        action: Action::Read,
        account: source_id.map(str::to_owned),
        mailbox: None,
        message: None,
        now: time::OffsetDateTime::now_utc(),
    };
    match crate::authz::evaluate(&caveats, &ctx) {
        crate::authz::Decision::Allow => Ok(()),
        crate::authz::Decision::Deny(_) => Err(ApiError::new(
            StatusCode::FORBIDDEN,
            ApiErrorCode::Forbidden,
            "token is not authorized for this request",
        )),
    }
}

fn runtime_caller(source_id: Option<&str>) -> RuntimeCaller {
    let mut caller = RuntimeCaller::api();
    caller.account_scope = source_id.map(|source_id| vec![source_id.to_string()]);
    caller
}

fn frame_to_sse(frame: RuntimeFrame) -> Result<Event, Infallible> {
    Ok(Event::default()
        .id(frame.session_seq().get().to_string())
        .json_data(frame)
        .unwrap_or_else(|_| Event::default().data("{}")))
}
