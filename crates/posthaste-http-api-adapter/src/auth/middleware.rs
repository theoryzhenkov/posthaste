use std::sync::Arc;

use axum::extract::{MatchedPath, Request, State};
use axum::http::header;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::auth::context::build_context;
use crate::auth::errors::{forbidden, forbidden_scope, misconfigured, unauthorized};
use crate::auth::perimeter::{bearer_token, host_allowed, is_exempt_path, origin_allowed};
use crate::auth::PresentedToken;
use crate::authz::{self, Decision};
use crate::token::{self, TokenError};
use crate::AppState;

/// Axum middleware enforcing the loopback trust model on the `/v1` api router.
///
/// Pass-through when `state.require_auth` is `false`. Otherwise, in order:
/// - validate the `Host` header against the allowlist on **every** request,
///   before any exemption (the DNS-rebinding defense);
/// - exempt liveness/doc routes from token auth;
/// - reject browser requests whose `Origin`/`Referer` is not allowlisted;
/// - require an authentic macaroon (HMAC chain verified against the root key)
///   presented in the `Authorization` header — there is no query-param token;
/// - enforce the macaroon's first-party caveats against the matched route's
///   authz descriptor (Stage B): a forged/garbled token is 401, an authentic
///   token whose caveats are out of scope is 403, and a scoped token on an
///   unmapped route fails closed (500).
///
/// @spec docs/eph/DESIGN-L1-trust-model
/// @spec docs/eph/DESIGN-L1-capability-tokens
pub async fn require_auth_layer(
    State(state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Response {
    if !state.require_auth {
        return next.run(req).await;
    }

    // Host validation runs first, for ALL routes including the exempt ones. A
    // DNS-rebinding attack arrives as `Host: attacker.com` with no `Origin`, so
    // the Origin check alone is insufficient; a mandatory Host allowlist is the
    // real defense. A missing Host is also rejected.
    if !host_allowed(&req, &state.host_allowlist) {
        return forbidden().into_response();
    }

    let path = req.uri().path().to_string();
    if is_exempt_path(&path) {
        return next.run(req).await;
    }

    // Origin/Referer defense-in-depth (CSRF): a token can be read from the page
    // context under rebinding, so a browser-supplied Origin/Referer must also be
    // allowlisted. Non-browser clients send neither and pass on token alone.
    let origin_header = req
        .headers()
        .get(header::ORIGIN)
        .or_else(|| req.headers().get(header::REFERER))
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if let Some(origin) = origin_header {
        if !origin_allowed(&origin, &state.origin_allowlist) {
            return forbidden().into_response();
        }
    }

    // Bearer token from the Authorization header. Every client — including the
    // SSE stream (`fetchEventSource`) and the browser-loadable logo/attachment
    // fetches (blob fetch) — sends the header; the token never rides in a query
    // param, so there is no `access_token` fallback to honor.
    let Some(presented) = bearer_token(&req).map(str::to_owned) else {
        return unauthorized().into_response();
    };

    // 1. Authenticity (signature) check. A forged/garbled token is 401; an
    //    authentic-but-attenuated token returns its caveats for enforcement.
    let caveats = match token::verify_authenticity(&presented, &state.macaroon_root_key) {
        Ok(caveats) => caveats,
        Err(TokenError::Malformed) | Err(TokenError::BadSignature) => {
            return unauthorized().into_response();
        }
    };

    // Stash the verified token so a handler that derives a narrower token from
    // it (the mint endpoint, `POST /v1/auth/tokens`) can attenuate THIS token —
    // attenuation only narrows, so a minted token can never exceed the caller's
    // authority, regardless of what scope it requests.
    req.extensions_mut()
        .insert(PresentedToken(presented.clone()));

    // Fast path: a full-scope macaroon (no caveats) is authorized everywhere,
    // exactly as before — no authz-map lookup needed.
    if caveats.is_empty() {
        return next.run(req).await;
    }

    // 2. Resolve the matched route's authz descriptor. `MatchedPath` carries the
    //    FULL template including the `/v1` nest prefix (axum appends the nested
    //    router's template to the outer mount), so strip it to the nest-stripped
    //    template the authz map is keyed on (e.g. `/sources/{source_id}/messages`).
    let Some(matched) = req.extensions().get::<MatchedPath>().map(|m| {
        m.as_str()
            .strip_prefix("/v1")
            .unwrap_or(m.as_str())
            .to_owned()
    }) else {
        // No matched path means no route matched (404 fallback). An attenuated
        // token on a non-route is denied; let routing return its own status by
        // failing closed here.
        return forbidden_scope().into_response();
    };
    let method = req.method().as_str().to_owned();
    let Some(route_authz) = authz::lookup(&method, &matched) else {
        // Unmapped route + scoped token → fail CLOSED as misconfiguration. A new
        // route without an authz entry must never ship open. (Full-scope tokens
        // took the fast path above, so this only blocks attenuated tokens.)
        return misconfigured().into_response();
    };

    // 3. Build the caveat context (path params; query filter for Filter routes).
    let ctx = build_context(&req, &matched, &route_authz);

    // 4. Evaluate every caveat. Any unsatisfied caveat → 403 (authentic but out
    //    of scope), distinct from the 401 above.
    match authz::evaluate(&caveats, &ctx) {
        Decision::Allow => next.run(req).await,
        Decision::Deny(reason) => {
            // D72: the deny reason (non-sensitive: which caveat/axis was out of
            // scope) is why authz refused — log it instead of discarding it.
            log_authz_denied(&format!("{method} {matched}"), &reason);
            forbidden_scope().into_response()
        }
    }
}

/// Log an authz caveat denial at the `/v1` boundary (D72). The reason is a
/// non-sensitive scope description (e.g. "mailbox out of scope", "token
/// expired") — operators need it to diagnose an over-attenuated token.
pub(crate) fn log_authz_denied(route: &str, reason: &str) {
    posthaste_observability::ph_warn!(
        posthaste_observability::events::HTTP_AUTHZ_DENIED,
        route = %route,
        reason = %reason,
        "authz denied /v1 request"
    );
}

/// The shared token→status + deny→status mapping for handler-side caveat checks.
///
/// Routes that authorize *after* the middleware — the runtime-stream SSE and
/// mutation handlers, which evaluate caveats against a route-specific
/// [`authz::CaveatContext`] the middleware cannot build — call this instead of
/// hand-duplicating the mapping. It mirrors [`require_auth_layer`] exactly so the
/// two never drift: an absent or forged token is 401 ([`unauthorized`]); a
/// full-scope token (no caveats) passes; an out-of-scope caveat is 403
/// ([`forbidden_scope`]) with the deny reason logged (D72).
pub(crate) fn authorize_presented_caveats(
    presented: Option<&PresentedToken>,
    macaroon_root_key: &token::RootKey,
    ctx: &authz::CaveatContext,
    route: &str,
) -> Result<(), crate::api::ApiError> {
    let Some(presented) = presented else {
        return Err(unauthorized());
    };
    let caveats = token::verify_authenticity(&presented.0, macaroon_root_key)
        .map_err(|_| unauthorized())?;
    if caveats.is_empty() {
        return Ok(());
    }
    match authz::evaluate(&caveats, ctx) {
        Decision::Allow => Ok(()),
        Decision::Deny(reason) => {
            log_authz_denied(route, &reason);
            Err(forbidden_scope())
        }
    }
}
