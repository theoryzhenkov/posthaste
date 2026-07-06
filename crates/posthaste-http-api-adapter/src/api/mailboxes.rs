use super::*;

/// Request body for `PATCH /v1/sources/{source_id}/mailboxes/{mailbox_id}`.
///
/// Outer `Option` distinguishes omitted `role` from an explicit JSON `null`.
///
/// @spec docs/L1-api#conversations-and-messages
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatchMailboxRequest {
    // `double_option` is load-bearing: serde otherwise collapses both an absent
    // field and an explicit `null` to `None`, so `{"role": null}` would be
    // rejected as "role is required" and a mailbox role could never be cleared
    // from the UI. With it, absent -> None (unchanged) and null -> Some(None)
    // (clear).
    #[serde(default, deserialize_with = "double_option")]
    pub role: Option<Option<String>>,
}

/// Deserialize a present field (even an explicit `null`) as `Some`, so the outer
/// `Option` can tell "omitted" (`None`, via `#[serde(default)]`) from "explicit
/// null" (`Some(None)`). Without this, serde maps both to `None`.
fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(deserializer).map(Some)
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
    state
        .runtime
        .set_mailbox_role(
            RuntimeCaller::api(),
            AccountId(source_id),
            MailboxId(mailbox_id),
            role,
        )
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

/// Request body for `POST /v1/sources/{source_id}/mailboxes`.
///
/// Flat create — a `name` only; the parent/hierarchy is out of scope.
///
/// @spec docs/eph/RFC-L2-mailbox-management
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateMailboxRequest {
    pub name: String,
}

/// POST /v1/sources/{source_id}/mailboxes
///
/// @spec docs/eph/RFC-L2-mailbox-management
/// @spec docs/L1-jmap#methods-used
#[utoipa::path(
    post,
    path = "/v1/sources/{source_id}/mailboxes",
    tag = "mailboxes",
    summary = "Create mailbox",
    description = "Creates a new top-level mailbox and returns the source's refreshed mailbox list.",
    params(("source_id" = String, Path, description = "Source (account) identifier")),
    request_body = CreateMailboxRequest,
    responses(
        (status = 200, description = "Updated mailboxes for the source", body = [MailboxSummary]),
        (status = 400, description = "Invalid mailbox name", body = ApiErrorBody),
        (status = 404, description = "Source not found", body = ApiErrorBody),
        (status = 503, description = "Account gateway unavailable", body = ApiErrorBody)
    )
)]
pub async fn create_mailbox(
    State(state): State<Arc<AppState>>,
    Path(source_id): Path<String>,
    Json(request): Json<CreateMailboxRequest>,
) -> Result<Json<Vec<MailboxSummary>>, ApiError> {
    let name = validate_create_mailbox_name(request.name)?;
    state
        .runtime
        .create_mailbox(RuntimeCaller::api(), AccountId(source_id), name)
        .await
        .map(Json)
        .map_err(ApiError::from_runtime_error)
}

fn validate_create_mailbox_name(name: String) -> Result<String, ApiError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidMailbox,
            "mailbox name is required",
        ));
    }
    Ok(trimmed.to_string())
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

#[cfg(test)]
mod patch_mailbox_role_tests {
    use super::*;

    fn role_of(json: &str) -> Option<Option<String>> {
        serde_json::from_str::<PatchMailboxRequest>(json)
            .expect("valid JSON")
            .role
    }

    #[test]
    fn explicit_null_role_clears_and_is_not_rejected() {
        // {"role": null} must clear the role (Ok(None)), not error "role is
        // required" — the bug that blocked setting a mailbox role to None.
        let role = role_of(r#"{"role": null}"#);
        assert_eq!(
            role,
            Some(None),
            "explicit null must deserialize as Some(None)"
        );
        assert_eq!(validate_patch_mailbox_role(role).ok(), Some(None));
    }

    #[test]
    fn omitted_role_is_required() {
        let role = role_of(r"{}");
        assert_eq!(role, None, "omitted role is outer None");
        assert!(validate_patch_mailbox_role(role).is_err());
    }

    #[test]
    fn valid_role_is_accepted() {
        let role = role_of(r#"{"role": "archive"}"#);
        assert_eq!(
            validate_patch_mailbox_role(role).ok(),
            Some(Some("archive".to_string()))
        );
    }

    #[test]
    fn unsupported_role_is_rejected() {
        assert!(validate_patch_mailbox_role(role_of(r#"{"role": "nonsense"}"#)).is_err());
    }
}
