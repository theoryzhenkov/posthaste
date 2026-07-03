//! The **read-only** automation-rules REST surface (RFC-L2-scripting §7.18).
//!
//! Rules are config-file-only for beta: authored by editing `rules.toml` on the
//! host. There is deliberately **no REST write path** — a REST-settable `exec`
//! action would be remote code execution (§7.16). This bundled-only route lists
//! the loaded rules so a UI/operator can see what is configured. Drafting a
//! rule's WHEN-clause is previewed through the existing
//! `POST /v1/automation-rules:preview` (a `Rule.when` is a `SmartMailboxRule`).
//!
//! It lives in the composition root (not the near `posthaste-http-api-adapter`
//! platform) because it reads rules through `posthaste-authority-server`; the
//! lean remote runtime daemon, which has no local authority server, does not
//! serve it — exactly like the OAuth routes.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use utoipa::ToSchema;

use posthaste_domain_model::Rule;
use posthaste_http_api_adapter::api::{ApiError, ApiErrorBody, ApiErrorCode};
use posthaste_http_api_adapter::AppState;

/// The read-only automation-rules listing.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RulesListResponse {
    /// The rules loaded from `rules.toml`, in file order.
    pub rules: Vec<Rule>,
}

/// GET /v1/rules — list the config-file-authored automation rules (read-only).
///
/// @spec docs/eph/RFC-L2-scripting#7-rulings
#[utoipa::path(
    get,
    path = "/v1/rules",
    tag = "settings",
    summary = "List automation rules",
    description = "Lists the automation rules loaded from the host's rules.toml. Read-only: rules \
are config-file-only (there is no REST write path — a REST-settable exec action would be remote \
code execution). Draft WHEN-clauses are previewed via POST /v1/automation-rules:preview.",
    responses(
        (status = 200, description = "The configured rules", body = RulesListResponse),
        (status = 500, description = "rules.toml could not be read", body = ApiErrorBody)
    )
)]
pub async fn list_rules(
    State(state): State<Arc<AppState>>,
) -> Result<Json<RulesListResponse>, ApiError> {
    let rules = posthaste_authority_server::load_rules(&state.config_root).map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::InternalError,
            error.to_string(),
        )
    })?;
    Ok(Json(RulesListResponse { rules }))
}

/// Build the read-only rules sub-router behind the SAME macaroon perimeter the
/// `/v1` API router uses (uniform auth on the merged surface).
pub fn build_rules_router(app: Arc<AppState>) -> Router {
    Router::new()
        .route("/rules", get(list_rules))
        .layer(middleware::from_fn_with_state(
            app.clone(),
            posthaste_http_api_adapter::auth::require_auth_layer,
        ))
        .with_state(app)
}
