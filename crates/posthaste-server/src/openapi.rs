//! Bundled-server OpenAPI document.
//!
//! The bundled/backend server serves the near `/v1` platform PLUS the OAuth-flow
//! routes (which need the provider machinery in the far crate). [`document`]
//! merges the near document with those routes, and is the source for the
//! committed `openapi.json` contract + the runtime `/v1/openapi.json` the bundled
//! server serves. A lean near node serves `posthaste_api::openapi::document()`
//! (no OAuth) instead.

use utoipa::OpenApi;

// The contract-doc serving helpers are deployment-neutral — reuse the near ones.
pub use posthaste_api::openapi::{asyncapi_json, openapi_router};

/// The OAuth-flow routes the bundled server layers on top of the near platform.
#[derive(OpenApi)]
#[openapi(
    paths(
        crate::oauth_routes::handlers::start_account_oauth,
        crate::oauth_routes::handlers::start_provider_oauth,
        crate::oauth_routes::handlers::complete_account_oauth,
    ),
    components(schemas(
        posthaste_api::api::StartOAuthRequest,
        posthaste_api::api::StartProviderOAuthRequest,
        posthaste_api::api::StartOAuthResponse,
    )),
    tags((name = "oauth", description = "Provider OAuth authorization flows")),
)]
struct OAuthApiDoc;

/// The full bundled-server OpenAPI document: the near `/v1` platform plus the
/// OAuth-flow routes. This is the committed `openapi.json` contract artifact.
pub fn document() -> utoipa::openapi::OpenApi {
    let mut doc = posthaste_api::openapi::document();
    doc.merge(OAuthApiDoc::openapi());
    doc
}
