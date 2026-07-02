use super::*;

/// Request body for `POST /v1/auth/tokens`: the scope a derived capability token
/// should carry. Every field NARROWS authority — the minted token is the
/// caller's own token with these caveats appended (attenuation), so it can never
/// exceed what the caller already holds. All fields are optional; an empty
/// request returns a token equivalent in authority to the caller's.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAuthTokenRequest {
    /// Restrict the token to these actions (subset of
    /// `read,send,tag,move,delete,manage`). Omitted = no added action caveat.
    pub actions: Option<Vec<Action>>,
    /// Restrict the token to a single account (`source_id`).
    pub account: Option<String>,
    /// Restrict the token to a single mailbox.
    pub mailbox: Option<String>,
    /// Restrict the token to a single message.
    pub message: Option<String>,
    /// Token lifetime in seconds from now. Omitted = no expiry caveat (lives as
    /// long as the root key). Recommended for shared/agent tokens.
    pub expires_in_seconds: Option<u64>,
}

/// Response for `POST /v1/auth/tokens`.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAuthTokenResponse {
    /// The minted capability token (a macaroon), for use as
    /// `Authorization: Bearer <token>`.
    pub token: String,
    /// RFC3339 UTC expiry, present iff `expiresInSeconds` was set.
    pub expires_at: Option<String>,
}

/// The caveat value to use for a resource axis: the field verbatim if it has
/// non-whitespace content, else `None`. NOT trimmed — the caveat is compared for
/// exact equality against the request's path value, so the value must match what
/// the client will send on the path.
fn caveat_value(field: &Option<String>) -> Option<&str> {
    field.as_deref().filter(|value| !value.trim().is_empty())
}

/// Translate a validated mint request into caveat predicate strings (the
/// documented `authz` format) plus the resolved RFC3339 expiry. Returns 400 on
/// an empty `actions` list or a zero/overflowing lifetime.
pub(super) fn build_token_caveats(
    request: &CreateAuthTokenRequest,
    now: time::OffsetDateTime,
) -> Result<(Vec<String>, Option<String>), ApiError> {
    let bad_request = |message: &str| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidQuery,
            message.to_string(),
        )
    };

    let mut predicates = Vec::new();

    if let Some(actions) = &request.actions {
        if actions.is_empty() {
            return Err(bad_request("actions must not be empty when provided"));
        }
        let verbs = actions
            .iter()
            .map(|action| action.as_str())
            .collect::<Vec<_>>()
            .join(",");
        predicates.push(format!("action = {verbs}"));
    }
    if let Some(account) = caveat_value(&request.account) {
        predicates.push(format!("account = {account}"));
    }
    if let Some(mailbox) = caveat_value(&request.mailbox) {
        predicates.push(format!("mailbox = {mailbox}"));
    }
    if let Some(message) = caveat_value(&request.message) {
        predicates.push(format!("message = {message}"));
    }

    let expires_at = match request.expires_in_seconds {
        None => None,
        Some(0) => return Err(bad_request("expiresInSeconds must be greater than zero")),
        Some(seconds) => {
            let seconds =
                i64::try_from(seconds).map_err(|_| bad_request("expiresInSeconds is too large"))?;
            let expiry = now
                .checked_add(time::Duration::seconds(seconds))
                .ok_or_else(|| bad_request("expiresInSeconds is too large"))?;
            let formatted = expiry
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| internal_error("failed to format token expiry".to_string()))?;
            predicates.push(format!("expires = {formatted}"));
            Some(formatted)
        }
    };

    Ok((predicates, expires_at))
}

/// Mint a narrower capability token. The handler **attenuates the caller's own
/// token** (adding the requested caveats), so the result can only narrow — never
/// widen — the caller's authority, whatever scope is requested. The route is
/// `Manage`-gated with no resource axis, so only a full-scope (or unscoped
/// `manage`) token reaches here; resource-scoped tokens are rejected (403)
/// before the handler runs.
#[utoipa::path(
    post,
    path = "/v1/auth/tokens",
    tag = "auth",
    summary = "Mint a capability token",
    description = "Derives a narrower capability token from the caller's token by appending the \
requested caveats (attenuation). The minted token can only narrow the caller's authority, never \
widen it. Requires a full-scope (or unscoped `manage`) token.",
    request_body = CreateAuthTokenRequest,
    responses(
        (status = 200, description = "The minted capability token", body = CreateAuthTokenResponse),
        (status = 400, description = "Invalid scope request", body = ApiErrorBody),
        (status = 403, description = "Caller token is not authorized to mint", body = ApiErrorBody)
    )
)]
pub async fn create_auth_token(
    State(state): State<Arc<AppState>>,
    presented: Option<Extension<crate::auth::PresentedToken>>,
    Json(request): Json<CreateAuthTokenRequest>,
) -> Result<Json<CreateAuthTokenResponse>, ApiError> {
    let now = time::OffsetDateTime::now_utc();
    let (predicates, expires_at) = build_token_caveats(&request, now)?;
    let caller = presented.map(|Extension(crate::auth::PresentedToken(token))| token);
    let token = derive_capability_token(caller, &state.macaroon_root_key, &predicates)?;
    Ok(Json(CreateAuthTokenResponse { token, expires_at }))
}

/// Produce the minted token from the requested caveat predicates.
///
/// With a `caller` token (the normal, authenticated case) this **attenuates the
/// caller's own token**: attenuation can only ADD caveats, which AND together,
/// so the result is always ≤ the caller's authority — never wider, whatever was
/// requested. Without a caller (`require_auth` off, no token to preserve) it
/// mints from the root key with the requested caveats.
pub(super) fn derive_capability_token(
    caller: Option<String>,
    root: &crate::token::RootKey,
    predicates: &[String],
) -> Result<String, ApiError> {
    match caller {
        Some(caller) => {
            let mut token = caller;
            for predicate in predicates {
                token = crate::token::attenuate(&token, predicate).map_err(|_| {
                    internal_error("failed to attenuate capability token".to_string())
                })?;
            }
            Ok(token)
        }
        None => {
            let refs: Vec<&str> = predicates.iter().map(String::as_str).collect();
            Ok(crate::token::mint_with_caveats(root, &refs))
        }
    }
}
