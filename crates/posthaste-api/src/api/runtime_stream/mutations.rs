use super::*;

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
