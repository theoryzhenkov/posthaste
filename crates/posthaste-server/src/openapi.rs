//! OpenAPI document for the Posthaste REST surface.
//!
//! The Rust handlers are the single source of truth: each handler is annotated
//! with `#[utoipa::path]` and each wire type derives `ToSchema`. [`ApiDoc`]
//! aggregates them into one document, served at `GET /v1/openapi.json` and
//! emitted to the committed `openapi.json` contract artifact.
//!
//! @spec docs/L1-api#openapi-contract

use axum::Json;
use utoipa::OpenApi;

/// Aggregated OpenAPI document for the `/v1` REST surface.
///
/// As handlers are annotated during P1, register each one under `paths(...)`
/// and each wire type under `components(schemas(...))`.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Posthaste API",
        version = "0.1.0",
        license(name = "MIT", identifier = "MIT"),
        description = "Local-first JMAP mail client backend. The `/v1` surface is the \
                       documented, versioned contract for first-party clients, custom \
                       clients, and agents."
    ),
    paths(
        crate::api::health,
    ),
    components(schemas(
        crate::api::HealthResponse,
        crate::api::ApiErrorBody,
    )),
    tags(
        (name = "system", description = "Health and service status")
    )
)]
pub struct ApiDoc;

/// Generate the OpenAPI document. Single entry point for both the served route
/// and the committed-artifact contract test.
pub fn document() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

/// `GET /v1/openapi.json` — serve the generated OpenAPI document.
pub async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(document())
}
