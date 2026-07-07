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

/// Mint a capability token. For most callers the handler **attenuates the
/// caller's own token** (adding the requested caveats), so the result can only
/// narrow — never widen — the caller's authority, whatever scope is requested.
///
/// The one exception is a caller whose OWN token holds the `mint` action (RFC-
/// L2-scripting §7 ruling 11): `mint` is an issuance right, not a substantive
/// scope, so such a caller mints FRESH from the server's root key instead —
/// this is what lets the least-default discovery bootstrap (`{mint, tap:read}`,
/// which has no write scope of its own to narrow FROM) transparently obtain
/// write-capable tokens via `posthastectl token mint`. See
/// `derive_capability_token`/`caller_grants_mint` below.
///
/// The route is `Mint`-gated with no resource axis, so only a full-scope (or
/// unscoped, mint-carrying) token reaches here; resource-scoped tokens are
/// rejected (403) before the handler runs.
#[utoipa::path(
    post,
    path = "/v1/auth/tokens",
    tag = "auth",
    summary = "Mint a capability token",
    description = "Derives a capability token from the caller's token. For most callers this \
appends the requested caveats (attenuation) and can only narrow authority, never widen it. A \
caller holding the `mint` action is the exception: it mints fresh from the root key, so it can \
obtain a WIDER token (the discovery bootstrap trades this for a working token via `token mint`). \
Requires a full-scope (or unscoped, mint-carrying) token.",
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

/// Whether `token`'s OWN first-party caveats grant the `mint` action — i.e.
/// whether it is authorized to reach `POST /v1/auth/tokens` with no resource
/// axis, the exact test the route's authz gate applies (built by simulating
/// that gate: [`crate::authz::evaluate`] against an unrestricted `Mint`
/// context). An inauthentic/malformed token or one that fails the check is
/// treated as NOT mint-holding — fail closed. Redundant with the route's own
/// authz gate (which already required this to reach the handler at all); kept
/// as a real check, not assumed, so this function stays correct if ever called
/// outside the route.
///
/// @spec docs/eph/RFC-L2-scripting#7-rulings
fn caller_grants_mint(token: &str, root: &crate::token::RootKey) -> bool {
    let Ok(caveats) = crate::token::verify_authenticity(token, root) else {
        return false;
    };
    if caveats.is_empty() {
        // Full-scope: unrestricted, but not a `mint`-CARRYING token — it takes
        // the ordinary attenuation path below (equivalent in effect, since
        // attenuating an empty-caveat token is the same restriction set as
        // minting fresh with those same caveats).
        return false;
    }
    let ctx = crate::authz::CaveatContext {
        action: Some(Action::Mint),
        account: None,
        mailbox: None,
        message: None,
        now: time::OffsetDateTime::now_utc(),
    };
    matches!(
        crate::authz::evaluate(&caveats, &ctx),
        crate::authz::Decision::Allow
    )
}

/// Produce the minted token from the requested caveat predicates.
///
/// With a `caller` token (the normal, authenticated case) this **attenuates the
/// caller's own token**: attenuation can only ADD caveats, which AND together,
/// so the result is always ≤ the caller's authority — never wider, whatever was
/// requested. Without a caller (`require_auth` off, no token to preserve) it
/// mints from the root key with the requested caveats.
///
/// **Exception**: a caller whose own token grants the `mint` action
/// ([`caller_grants_mint`]) mints FRESH from the root key too, exactly like the
/// no-caller case, INSTEAD of attenuating. `mint` is an issuance right, not a
/// substantive scope — the discovery bootstrap is `{mint, tap:read}` and has no
/// write scope of its own to narrow from, so attenuating it could only ever
/// stack an unsatisfiable second `action` restriction (two `action = ...`
/// caveats AND together, i.e. intersect). Treating `mint` as "authorized to
/// call the token factory" rather than "a scope to narrow" is what lets
/// `posthastectl token mint --grant apply,...` work transparently against the
/// bootstrap (RFC-L2-scripting §7 ruling 11). A caller withOUT `mint` (e.g. a
/// plain `action = manage` token) is unaffected and still only ever narrows.
pub(super) fn derive_capability_token(
    caller: Option<String>,
    root: &crate::token::RootKey,
    predicates: &[String],
) -> Result<String, ApiError> {
    match caller {
        Some(caller) if caller_grants_mint(&caller, root) => {
            let refs: Vec<&str> = predicates.iter().map(String::as_str).collect();
            Ok(crate::token::mint_with_caveats(root, &refs))
        }
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
