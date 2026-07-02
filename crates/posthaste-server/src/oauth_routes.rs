//! The OAuth provider-flow routes — the far half of `/v1`.
//!
//! Account creation/linking via a provider's PKCE authorization flow needs the
//! authority server's provider machinery (token exchange, JWKS, the account-mutation
//! service), so it lives here rather than in the near `posthaste-http-api-adapter` platform.
//! The lean remote runtime daemon does not serve these routes; the bundled
//! server merges [`build_oauth_router`] into its `/v1` router.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::middleware;
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use posthaste_domain_model::{AccountConnectionOverview, AccountId, ServiceError};
use posthaste_contract_core::{RuntimeCaller, RuntimeError};
use posthaste_runtime_api::RuntimeAccountApi;

use axum::http::StatusCode;
use posthaste_http_api_adapter::api::{
    ApiError, ApiErrorBody, ApiErrorCode, OAuthCallbackQuery, StartOAuthRequest,
    StartOAuthResponse, StartProviderOAuthRequest,
};
use posthaste_http_api_adapter::AppState;
use posthaste_authority_server::oauth::{
    OAuthAuthorizationCodeExchange, OAuthExchangeResult, OAuthFlowCompletion, OAuthFlowStore,
    OAuthProviderProfile, OAuthTokenService, OAuthTokenSet, PendingOAuthFlow,
};
use posthaste_authority_server::AccountMutationService;

pub(crate) mod handlers;
mod support;
#[cfg(test)]
mod tests;

/// State for the OAuth routes: the near `/v1` app state (runtime handle + auth)
/// plus the authority-server-only OAuth machinery (the pending-flow store and the
/// account-mutation service for the holdout).
#[derive(Clone)]
pub struct OAuthState {
    pub app: Arc<AppState>,
    pub oauth_flows: Arc<OAuthFlowStore>,
    /// `None` on a remote near node (no local authority server) — the holdout then
    /// returns the "account mutation runtime is not available" error.
    pub oauth_mutations: Option<Arc<AccountMutationService>>,
}

/// Build the OAuth route sub-router, finalized with its state and the SAME
/// macaroon auth perimeter the `/v1` API router uses (so the merged surface
/// enforces auth uniformly; `/oauth/callback` stays perimeter-exempt).
pub fn build_oauth_router(state: Arc<OAuthState>) -> Router {
    Router::new()
        .route(
            "/accounts/{account_id}/oauth/start",
            post(handlers::start_account_oauth),
        )
        .route("/oauth/start", post(handlers::start_provider_oauth))
        .route("/oauth/callback", get(handlers::complete_account_oauth))
        .layer(middleware::from_fn_with_state(
            state.app.clone(),
            posthaste_http_api_adapter::auth::require_auth_layer,
        ))
        .with_state(state)
}
