use super::*;

#[utoipa::path(
    post,
    path = "/v1/runtime/sessions/{session_id}/mutations",
    tag = "runtime",
    summary = "Run a runtime mutation",
    description = "Submits a named mutation to a runtime link (message read/flag/tags/move/destroy) and emits mutationSettlement RuntimeFrame values on the link stream.",
    params(
        ("session_id" = String, Path, description = "Runtime link id"),
        RuntimeLinkQuery
    ),
    request_body = MutationRequest,
    responses(
        (status = 200, description = "Mutation receipt", body = MutationReceipt),
        (status = 400, description = "Invalid mutation", body = ApiErrorBody),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 403, description = "Forbidden", body = ApiErrorBody),
        (status = 404, description = "Unknown runtime link", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn run_runtime_link_mutation(
    State(state): State<Arc<AppState>>,
    Path(link_id): Path<String>,
    Query(query): Query<RuntimeLinkQuery>,
    presented: Option<Extension<crate::auth::PresentedToken>>,
    Json(mut request): Json<MutationRequest>,
) -> Result<Json<MutationReceipt>, ApiError> {
    let path_link_id = RuntimeLinkId::new(link_id);
    if request
        .link_id
        .as_ref()
        .is_some_and(|body_link_id| body_link_id != &path_link_id)
    {
        return Err(ApiError::from_runtime_error(
            RuntimeError::invalid_mutation("request link id does not match path link id"),
        ));
    }
    request.link_id = Some(path_link_id);
    require_read_for_link_mutation(
        state.as_ref(),
        query.source_id.as_deref(),
        presented.as_ref().map(|Extension(token)| token),
    )?;
    state
        .runtime
        .forward_mutation(runtime_caller(query.source_id.as_deref()), request)
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

#[utoipa::path(
    get,
    path = "/v1/runtime/sessions/{session_id}/mutations/{client_mutation_id}",
    tag = "runtime",
    summary = "Read a runtime mutation's settlement",
    description = "Returns the settlement receipt the runtime holds for a client mutation id, or a null receipt when it has no record (unknown link, never accepted, or already cleared). The client near-end's reconciler queries this for sent-but-unsettled records after a link-continuity loss: a terminal receipt settles locally; a null receipt re-forwards.",
    params(
        ("session_id" = String, Path, description = "Runtime link id the mutation was dispatched under"),
        ("client_mutation_id" = String, Path, description = "Client mutation id"),
        RuntimeLinkQuery
    ),
    responses(
        (status = 200, description = "The runtime's settlement record (receipt is null when unknown)", body = RuntimeMutationSettlement),
        (status = 401, description = "Unauthorized", body = ApiErrorBody),
        (status = 403, description = "Forbidden", body = ApiErrorBody),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn runtime_link_mutation_settlement(
    State(state): State<Arc<AppState>>,
    Path((link_id, client_mutation_id)): Path<(String, String)>,
    Query(query): Query<RuntimeLinkQuery>,
    presented: Option<Extension<crate::auth::PresentedToken>>,
) -> Result<Json<RuntimeMutationSettlement>, ApiError> {
    require_read_for_link_mutation(
        state.as_ref(),
        query.source_id.as_deref(),
        presented.as_ref().map(|Extension(token)| token),
    )?;
    state
        .runtime
        .mutation_settlement(
            runtime_caller(query.source_id.as_deref()),
            RuntimeLinkId::new(link_id),
            posthaste_contract_core::ClientMutationId::new(client_mutation_id),
        )
        .await
        .map(|receipt| Json(RuntimeMutationSettlement { receipt }))
        .map_err(ApiError::from_runtime_error)
}

fn require_read_for_link_mutation(
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
