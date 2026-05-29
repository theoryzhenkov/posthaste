//! Contract test: the committed `openapi.json` must match the document the
//! server generates from its annotated handlers. The committed spec is the
//! source the TS client is generated from, so drift here means drift between
//! the backend and every generated client.
//!
//! Regenerate after intentional API changes with:
//!   `UPDATE_OPENAPI=1 cargo test -p posthaste-server --test openapi_contract`
//! (or the `just openapi` recipe).
//!
//! @spec docs/L1-api#openapi-contract

use std::path::PathBuf;

/// Repo-root `openapi.json` — the committed contract artifact.
fn committed_spec_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("openapi.json")
}

fn generated_spec() -> String {
    let mut json = posthaste_server::openapi::document()
        .to_pretty_json()
        .expect("OpenAPI document should serialize to JSON");
    json.push('\n');
    json
}

#[test]
fn committed_openapi_matches_generated() {
    let path = committed_spec_path();
    let generated = generated_spec();

    if std::env::var_os("UPDATE_OPENAPI").is_some() {
        std::fs::write(&path, &generated).expect("should write openapi.json");
        return;
    }

    let committed = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "{} is missing; generate it with `UPDATE_OPENAPI=1 cargo test -p posthaste-server \
             --test openapi_contract`",
            path.display()
        )
    });

    assert_eq!(
        committed, generated,
        "committed openapi.json is stale; regenerate with `UPDATE_OPENAPI=1 cargo test \
         -p posthaste-server --test openapi_contract`"
    );
}
