use super::*;

/// GET /v1/smart-mailboxes
///
/// @spec docs/L1-api#smart-mailboxes
#[utoipa::path(
    get,
    path = "/v1/smart-mailboxes",
    tag = "smart-mailboxes",
    summary = "List smart mailboxes",
    description = "Returns all smart mailboxes with live unread and total counts.",
    responses(
        (status = 200, description = "All smart mailboxes", body = [SmartMailboxSummary]),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn list_smart_mailboxes(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SmartMailboxSummary>>, ApiError> {
    state
        .runtime
        .list_smart_mailboxes(RuntimeCaller::api())
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// POST /v1/smart-mailboxes
///
/// Generates an ID from the name (`sm-{slug}-{uuid}`) and persists to config.
///
/// @spec docs/L1-api#smart-mailbox-crud
#[utoipa::path(
    post,
    path = "/v1/smart-mailboxes",
    tag = "smart-mailboxes",
    summary = "Create smart mailbox",
    description = "Generates an ID from the name and persists a new smart mailbox.",
    request_body = CreateSmartMailboxRequest,
    responses(
        (status = 200, description = "The created smart mailbox", body = SmartMailbox),
        (status = 400, description = "Validation failed", body = ApiErrorBody)
    )
)]
pub async fn create_smart_mailbox(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateSmartMailboxRequest>,
) -> Result<Json<SmartMailbox>, ApiError> {
    state
        .runtime
        .create_smart_mailbox(
            RuntimeCaller::api(),
            CreateSmartMailboxMutation {
                name: request.name,
                position: request.position,
                rule: request.rule,
            },
        )
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// GET /v1/smart-mailboxes/{id}
///
/// @spec docs/L1-api#smart-mailboxes
#[utoipa::path(
    get,
    path = "/v1/smart-mailboxes/{smart_mailbox_id}",
    tag = "smart-mailboxes",
    summary = "Get smart mailbox",
    description = "Returns a single smart mailbox with its rule.",
    params(("smart_mailbox_id" = String, Path, description = "Smart mailbox identifier")),
    responses(
        (status = 200, description = "The smart mailbox", body = SmartMailbox),
        (status = 404, description = "Smart mailbox not found", body = ApiErrorBody)
    )
)]
pub async fn get_smart_mailbox(
    State(state): State<Arc<AppState>>,
    Path(smart_mailbox_id): Path<String>,
) -> Result<Json<SmartMailbox>, ApiError> {
    state
        .runtime
        .get_smart_mailbox(RuntimeCaller::api(), SmartMailboxId::from(smart_mailbox_id))
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// PATCH /v1/smart-mailboxes/{id}
///
/// Merges name, position, and rule fields. Omitted fields are preserved.
///
/// @spec docs/L1-api#smart-mailbox-crud
#[utoipa::path(
    patch,
    path = "/v1/smart-mailboxes/{smart_mailbox_id}",
    tag = "smart-mailboxes",
    summary = "Update smart mailbox",
    description = "Merges name, position, and rule fields. Omitted fields are preserved.",
    params(("smart_mailbox_id" = String, Path, description = "Smart mailbox identifier")),
    request_body = PatchSmartMailboxRequest,
    responses(
        (status = 200, description = "The updated smart mailbox", body = SmartMailbox),
        (status = 400, description = "Validation failed", body = ApiErrorBody),
        (status = 404, description = "Smart mailbox not found", body = ApiErrorBody)
    )
)]
pub async fn patch_smart_mailbox(
    State(state): State<Arc<AppState>>,
    Path(smart_mailbox_id): Path<String>,
    Json(request): Json<PatchSmartMailboxRequest>,
) -> Result<Json<SmartMailbox>, ApiError> {
    state
        .runtime
        .patch_smart_mailbox(
            RuntimeCaller::api(),
            SmartMailboxId::from(smart_mailbox_id),
            PatchSmartMailboxMutation {
                name: request.name,
                position: request.position,
                rule: request.rule,
            },
        )
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// DELETE /v1/smart-mailboxes/{id}
///
/// @spec docs/L1-api#smart-mailboxes
#[utoipa::path(
    delete,
    path = "/v1/smart-mailboxes/{smart_mailbox_id}",
    tag = "smart-mailboxes",
    summary = "Delete smart mailbox",
    description = "Deletes a smart mailbox.",
    params(("smart_mailbox_id" = String, Path, description = "Smart mailbox identifier")),
    responses(
        (status = 200, description = "Smart mailbox deleted", body = OkResponse),
        (status = 404, description = "Smart mailbox not found", body = ApiErrorBody)
    )
)]
pub async fn delete_smart_mailbox(
    State(state): State<Arc<AppState>>,
    Path(smart_mailbox_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    state
        .runtime
        .delete_smart_mailbox(RuntimeCaller::api(), SmartMailboxId::from(smart_mailbox_id))
        .await
        .map(|()| Json(OkResponse { ok: true }))
        .map_err(ApiError::from_runtime_error)
}

/// POST /v1/smart-mailboxes:reset-defaults
///
/// Restores default smart mailboxes (Inbox, Archive, Drafts, Sent, Junk,
/// Trash, All Mail) and returns the full list.
///
/// @spec docs/L1-api#smart-mailbox-crud
/// @spec docs/L1-accounts#smart-mailbox-defaults
#[utoipa::path(
    post,
    path = "/v1/smart-mailboxes:reset-defaults",
    tag = "smart-mailboxes",
    summary = "Reset default smart mailboxes",
    description = "Restores the built-in default smart mailboxes and returns the full list.",
    responses(
        (status = 200, description = "All smart mailboxes after reset", body = [SmartMailboxSummary]),
        (status = 500, description = "Internal error", body = ApiErrorBody)
    )
)]
pub async fn reset_default_smart_mailboxes(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<SmartMailboxSummary>>, ApiError> {
    state
        .runtime
        .reset_default_smart_mailboxes(RuntimeCaller::api())
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}
