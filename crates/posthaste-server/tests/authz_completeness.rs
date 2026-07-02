//! CI completeness test (modeled on `openapi_contract`): every non-exempt
//! `operationId` in the committed `openapi.json` MUST have a `RouteAuthz` entry
//! in the authz map. A new route added without an authz entry fails here rather
//! than shipping open — the same drift-guard taste as the OpenAPI contract test.
//!
//! Exempt routes (`/health`, `/openapi.json`, `/asyncapi.json`,
//! `/oauth/callback`) are intentionally absent from the map (the perimeter
//! exempts them before the token check); this test treats them as
//! intentionally-exempt, not missing.
//!
//! It also checks the reverse direction: every authz-map entry corresponds to a
//! real OpenAPI operation, so the map cannot drift with stale rows.
//!
//! @spec docs/eph/DESIGN-L1-capability-tokens

use std::collections::HashSet;
use std::path::PathBuf;

use posthaste_http_api_adapter::authz;
use serde_json::Value;

/// Templates that are intentionally not in the authz map (perimeter-exempt).
/// Stored as the nest-stripped template the auth middleware would see.
const EXEMPT_TEMPLATES: &[&str] = &[
    "/health",
    "/openapi.json",
    "/asyncapi.json",
    "/oauth/callback",
];

fn committed_spec_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("openapi.json")
}

/// Convert an OpenAPI path (`/v1/sources/{source_id}/messages`) to the
/// nest-stripped template the auth middleware matches on
/// (`/sources/{source_id}/messages`).
fn nest_stripped(openapi_path: &str) -> String {
    openapi_path
        .strip_prefix("/v1")
        .unwrap_or(openapi_path)
        .to_string()
}

/// All `(METHOD, nest-stripped template)` pairs declared in the OpenAPI spec.
fn openapi_routes() -> Vec<(String, String)> {
    let spec: Value = serde_json::from_str(
        &std::fs::read_to_string(committed_spec_path()).expect("openapi.json should read"),
    )
    .expect("openapi.json should parse");
    let paths = spec["paths"].as_object().expect("paths object");
    let mut routes = Vec::new();
    for (path, methods) in paths {
        let template = nest_stripped(path);
        for method in ["get", "post", "patch", "delete", "put"] {
            if methods.get(method).is_some() {
                routes.push((method.to_ascii_uppercase(), template.clone()));
            }
        }
    }
    routes
}

#[test]
fn every_non_exempt_operation_has_an_authz_entry() {
    let mut missing = Vec::new();
    for (method, template) in openapi_routes() {
        if EXEMPT_TEMPLATES.contains(&template.as_str()) {
            continue;
        }
        if authz::lookup(&method, &template).is_none() {
            missing.push(format!("{method} {template}"));
        }
    }
    assert!(
        missing.is_empty(),
        "operations missing a RouteAuthz entry (add them to the authz map): {missing:#?}"
    );
}

#[test]
fn every_authz_entry_maps_to_a_real_operation() {
    let openapi: HashSet<(String, String)> = openapi_routes().into_iter().collect();
    let mut stale = Vec::new();
    for (method, template) in authz::mapped_routes() {
        let key = (method.to_string(), template.to_string());
        if !openapi.contains(&key) {
            stale.push(format!("{method} {template}"));
        }
    }
    assert!(
        stale.is_empty(),
        "authz-map entries with no matching OpenAPI operation (stale rows): {stale:#?}"
    );
}
