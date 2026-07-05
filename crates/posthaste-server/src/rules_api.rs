//! The automation-rules REST surface (RFC-L2-scripting §7.18 + ruling 23).
//!
//! * `GET /v1/rules` — list the **merged** ruleset: the hand-authored
//!   `rules.toml` (may contain exec) PLUS the GUI-managed `rules.d/*.toml`.
//!   Read-only view of everything the engine runs.
//! * `POST /v1/rules` — create a GUI-managed rule.
//! * `PUT /v1/rules/{rule_id}` — replace a GUI-managed rule.
//! * `DELETE /v1/rules/{rule_id}` — delete a GUI-managed rule.
//!
//! # The exec security gate (ruling 23)
//!
//! The write routes deserialize their body into
//! [`WritableRuleInput`], whose `action` is a
//! [`WritableRuleAction`](posthaste_domain_model::WritableRuleAction) — the
//! projection of [`RuleAction`] with **no exec variant**. A `{"kind":"exec",…}`
//! body is therefore UNREPRESENTABLE: it fails at the serde boundary (a 422)
//! before any handler runs. The exec-is-config-file-only invariant (a
//! REST-settable exec = RCE, threat 3) is thus enforced *structurally* by the
//! type system, not by a runtime `if kind == "exec"` check. Managed writes land
//! in `rules.d/` only; the authored `rules.toml` is never spliced.
//!
//! Authorization: the write routes are `Manage`-scoped in the authz route table,
//! so a read-scoped capability token is rejected (403) — only a manage-capable
//! (or full-scope) caller may create/edit/delete rules. A created rule
//! hot-reloads into the live evaluator via the [`ManagedRulesHandle`], so it
//! fires on the next matching event without a server restart.
//!
//! It lives in the composition root (not the near `posthaste-http-api-adapter`
//! platform) because it reads/writes rules through `posthaste-authority-server`;
//! the lean remote runtime daemon, which has no local authority server, serves
//! the read route but 503s the write routes.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::middleware;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use posthaste_authority_server::{ManagedRulesHandle, RuleWriteError};
use posthaste_domain_model::{Rule, SmartMailboxRule, WritableRuleAction};
use posthaste_http_api_adapter::api::{ApiError, ApiErrorBody, ApiErrorCode, OkResponse};
use posthaste_http_api_adapter::AppState;

/// Router state: the near `AppState` (for `config_root` + the auth perimeter)
/// plus the live managed-rules controller. `managed` is `None` on a lean remote
/// near node (no local rule engine) — the write routes then 503.
#[derive(Clone)]
pub struct RulesApiState {
    app: Arc<AppState>,
    managed: Option<ManagedRulesHandle>,
}

/// The read-only automation-rules listing.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RulesListResponse {
    /// The merged ruleset (`rules.toml` + `rules.d/*.toml`).
    pub rules: Vec<Rule>,
}

/// The write body for create/replace. Its `action` is a
/// [`WritableRuleAction`], which has NO exec variant — that is the structural
/// exec-exclusion gate (ruling 23). `when` is the shared query grammar's
/// [`SmartMailboxRule`] tree (the same the WHEN-clause builder emits).
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WritableRuleInput {
    /// Optional id on create (a UUID is minted when absent). Ignored on
    /// `PUT` — the path `{rule_id}` is authoritative there.
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    /// The WHEN-clause tree.
    pub when: SmartMailboxRule,
    /// Trigger topics; empty ⇒ the message-update default family.
    #[serde(default)]
    pub on: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// The action — exec is unrepresentable here (structural gate).
    pub action: WritableRuleAction,
}

fn default_enabled() -> bool {
    true
}

impl WritableRuleInput {
    /// Lift a validated write body into a domain [`Rule`] with the given id (the
    /// `action` is lifted from the exec-free [`WritableRuleAction`]).
    fn into_rule(self, id: String) -> Rule {
        Rule {
            id,
            name: self.name,
            when: self.when,
            on: self.on,
            action: self.action.into(),
            enabled: self.enabled,
        }
    }
}

/// GET /v1/rules — list the merged ruleset (read-only).
///
/// @spec docs/eph/RFC-L2-scripting#7-rulings
#[utoipa::path(
    get,
    path = "/v1/rules",
    tag = "settings",
    summary = "List automation rules",
    description = "Lists the merged automation ruleset: the hand-authored rules.toml plus the \
GUI-managed rules.d/*.toml. Read-only.",
    responses(
        (status = 200, description = "The configured rules", body = RulesListResponse),
        (status = 500, description = "rules could not be read", body = ApiErrorBody)
    )
)]
pub async fn list_rules(
    State(state): State<RulesApiState>,
) -> Result<Json<RulesListResponse>, ApiError> {
    let rules =
        posthaste_authority_server::load_rules(&state.app.config_root).map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiErrorCode::InternalError,
                error.to_string(),
            )
        })?;
    Ok(Json(RulesListResponse { rules }))
}

/// POST /v1/rules — create a GUI-managed rule (Manage-scoped).
///
/// @spec docs/eph/RFC-L2-scripting#7-rulings
#[utoipa::path(
    post,
    path = "/v1/rules",
    tag = "settings",
    summary = "Create an automation rule",
    description = "Creates a GUI-managed automation rule in rules.d/. The body's action cannot be \
exec — that variant is not representable in WritableRuleAction (a REST-settable exec would be \
remote code execution). The new rule hot-reloads into the live engine. Manage-scoped.",
    request_body = WritableRuleInput,
    responses(
        (status = 201, description = "The created rule", body = Rule),
        (status = 400, description = "Invalid rule", body = ApiErrorBody),
        (status = 409, description = "A rule with that id already exists", body = ApiErrorBody),
        (status = 503, description = "No local rule engine (remote near node)", body = ApiErrorBody)
    )
)]
pub async fn create_rule(
    State(state): State<RulesApiState>,
    Json(input): Json<WritableRuleInput>,
) -> Result<(StatusCode, Json<Rule>), ApiError> {
    let managed = require_engine(&state)?;
    let id = input
        .id
        .clone()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let rule = input.into_rule(id);
    let created = managed.create(rule).map_err(write_error)?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// PUT /v1/rules/{rule_id} — replace a GUI-managed rule (Manage-scoped).
///
/// @spec docs/eph/RFC-L2-scripting#7-rulings
#[utoipa::path(
    put,
    path = "/v1/rules/{rule_id}",
    tag = "settings",
    summary = "Replace an automation rule",
    description = "Replaces a GUI-managed rule (matched by the path id). Only rules.d rules are \
editable — a hand-authored rules.toml rule is not. exec is unrepresentable in the body. \
Manage-scoped.",
    params(("rule_id" = String, Path, description = "The managed rule id")),
    request_body = WritableRuleInput,
    responses(
        (status = 200, description = "The updated rule", body = Rule),
        (status = 400, description = "Invalid rule", body = ApiErrorBody),
        (status = 404, description = "No managed rule with that id", body = ApiErrorBody),
        (status = 503, description = "No local rule engine (remote near node)", body = ApiErrorBody)
    )
)]
pub async fn update_rule(
    State(state): State<RulesApiState>,
    Path(rule_id): Path<String>,
    Json(input): Json<WritableRuleInput>,
) -> Result<Json<Rule>, ApiError> {
    let managed = require_engine(&state)?;
    let rule = input.into_rule(rule_id);
    let updated = managed.update(rule).map_err(write_error)?;
    Ok(Json(updated))
}

/// DELETE /v1/rules/{rule_id} — delete a GUI-managed rule (Manage-scoped).
///
/// @spec docs/eph/RFC-L2-scripting#7-rulings
#[utoipa::path(
    delete,
    path = "/v1/rules/{rule_id}",
    tag = "settings",
    summary = "Delete an automation rule",
    description = "Deletes a GUI-managed rule (rules.d only). Manage-scoped.",
    params(("rule_id" = String, Path, description = "The managed rule id")),
    responses(
        (status = 200, description = "Deleted", body = OkResponse),
        (status = 404, description = "No managed rule with that id", body = ApiErrorBody),
        (status = 503, description = "No local rule engine (remote near node)", body = ApiErrorBody)
    )
)]
pub async fn delete_rule(
    State(state): State<RulesApiState>,
    Path(rule_id): Path<String>,
) -> Result<Json<OkResponse>, ApiError> {
    let managed = require_engine(&state)?;
    managed.delete(&rule_id).map_err(write_error)?;
    Ok(Json(OkResponse { ok: true }))
}

/// The managed controller, or a 503 on a lean remote near node.
fn require_engine(state: &RulesApiState) -> Result<&ManagedRulesHandle, ApiError> {
    state.managed.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            ApiErrorCode::InternalError,
            "rule writes require a local authority server (this is a remote near node)",
        )
    })
}

/// Map a persistence error onto an HTTP status/code.
fn write_error(error: RuleWriteError) -> ApiError {
    let (status, code) = match &error {
        RuleWriteError::Conflict(_) => (StatusCode::CONFLICT, ApiErrorCode::Conflict),
        RuleWriteError::NotFound(_) => (StatusCode::NOT_FOUND, ApiErrorCode::NotFound),
        RuleWriteError::UnsafeId(_) | RuleWriteError::Invalid(_) => {
            (StatusCode::BAD_REQUEST, ApiErrorCode::ConfigValidation)
        }
        RuleWriteError::Io(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiErrorCode::InternalError,
        ),
    };
    ApiError::new(status, code, error.to_string())
}

/// Build the rules sub-router behind the SAME macaroon perimeter the `/v1` API
/// router uses (uniform auth + the per-route authz map: GET is Read-scoped, the
/// writes are Manage-scoped).
pub fn build_rules_router(app: Arc<AppState>, managed: Option<ManagedRulesHandle>) -> Router {
    let state = RulesApiState {
        app: app.clone(),
        managed,
    };
    Router::new()
        .route("/rules", get(list_rules).post(create_rule))
        .route("/rules/{rule_id}", put(update_rule).delete(delete_rule))
        .layer(middleware::from_fn_with_state(
            app,
            posthaste_http_api_adapter::auth::require_auth_layer,
        ))
        .with_state(state)
}
