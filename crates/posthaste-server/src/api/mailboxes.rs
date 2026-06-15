use super::*;

/// Request body for `PATCH /v1/sources/{source_id}/mailboxes/{mailbox_id}`.
///
/// Outer `Option` distinguishes omitted `role` from an explicit JSON `null`.
///
/// @spec docs/L1-api#conversations-and-messages
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchMailboxRequest {
    pub role: Option<Option<String>>,
}

/// GET /v1/sources/{source_id}/mailboxes
///
/// @spec docs/L1-api#conversations-and-messages
#[utoipa::path(
    get,
    path = "/v1/sources/{source_id}/mailboxes",
    tag = "mailboxes",
    summary = "List mailboxes",
    description = "Returns all mailboxes for a source with unread and total counts.",
    params(("source_id" = String, Path, description = "Source (account) identifier")),
    responses(
        (status = 200, description = "Mailboxes for the source", body = [MailboxSummary]),
        (status = 404, description = "Source not found", body = ApiErrorBody)
    )
)]
pub async fn list_mailboxes(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
) -> Result<Json<Vec<MailboxSummary>>, ApiError> {
    let account_id = AccountId(source_id);
    state
        .runtime
        .list_mailboxes(
            RuntimeCaller::api(),
            AccountScopeRequest::Explicit {
                account_ids: vec![account_id.clone()],
            },
        )
        .await
        .map(|mut by_account_id| Json(by_account_id.remove(&account_id).unwrap_or_default()))
        .map_err(ApiError::from_runtime_error)
}

/// PATCH /v1/sources/{source_id}/mailboxes/{mailbox_id}
///
/// @spec docs/L1-api#conversations-and-messages
/// @spec docs/L1-jmap#methods-used
#[utoipa::path(
    patch,
    path = "/v1/sources/{source_id}/mailboxes/{mailbox_id}",
    tag = "mailboxes",
    summary = "Update mailbox role",
    description = "Sets or clears the role assigned to a mailbox and returns the updated list.",
    params(
        ("source_id" = String, Path, description = "Source (account) identifier"),
        ("mailbox_id" = String, Path, description = "Mailbox identifier")
    ),
    request_body = PatchMailboxRequest,
    responses(
        (status = 200, description = "Updated mailboxes for the source", body = [MailboxSummary]),
        (status = 400, description = "Invalid mailbox role", body = ApiErrorBody),
        (status = 404, description = "Source or mailbox not found", body = ApiErrorBody),
        (status = 503, description = "Account gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn patch_mailbox(
    State(state): State<Arc<AppState>>,
    Path((source_id, mailbox_id)): Path<(String, String)>,
    Json(request): Json<PatchMailboxRequest>,
) -> Result<Json<Vec<MailboxSummary>>, ApiError> {
    let role = validate_patch_mailbox_role(request.role)?;
    let account_id = AccountId(source_id);
    let gateway = live_gateway(state.as_ref(), &account_id).await?;
    let events = state
        .service
        .set_mailbox_role(
            &account_id,
            &MailboxId(mailbox_id),
            role.as_deref(),
            gateway.as_ref(),
        )
        .await
        .map_err(ApiError::from_service_error)?;
    state.publish_events(&events);
    state
        .service
        .list_mailboxes(&account_id)
        .map(Json)
        .map_err(ApiError::from_service_error)
}

fn validate_patch_mailbox_role(role: Option<Option<String>>) -> Result<Option<String>, ApiError> {
    let Some(role) = role else {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidMailbox,
            "role is required",
        ));
    };
    match role.as_deref() {
        None => Ok(role),
        Some(value) if MailboxRole::parse(value).is_some() => Ok(role),
        Some(_) => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidMailbox,
            "unsupported mailbox role",
        )),
    }
}
