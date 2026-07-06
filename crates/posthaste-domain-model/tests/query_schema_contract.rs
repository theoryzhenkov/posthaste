//! Contract test: the committed `query-schema.json` must match the document the
//! canonical Rust schema ([`posthaste_domain_model::query_schema_document`])
//! produces. That artifact is the source the web query registry is generated
//! from (`apps/web/scripts/gen-query-schema.ts`), so drift here means the web
//! `FIELD_REGISTRY` could disagree with the store compiler — the exact Rust↔TS
//! drift R5b removes.
//!
//! Regenerate after an intentional schema change with:
//!   `UPDATE_QUERY_SCHEMA=1 cargo test -p posthaste-domain-model --test query_schema_contract`
//!
//! @spec docs/eph/RFC-L2-query-schema.md#d4--one-canonical-field-schema

use std::path::PathBuf;

/// Repo-root `query-schema.json` — the committed contract artifact.
fn committed_artifact_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("query-schema.json")
}

#[test]
fn committed_query_schema_matches_generated() {
    let path = committed_artifact_path();
    let generated = posthaste_domain_model::query_schema_json();

    if std::env::var_os("UPDATE_QUERY_SCHEMA").is_some() {
        std::fs::write(&path, &generated).expect("should write query-schema.json");
        return;
    }

    let committed = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "{} is missing; generate it with `UPDATE_QUERY_SCHEMA=1 cargo test \
             -p posthaste-domain-model --test query_schema_contract`",
            path.display()
        )
    });

    assert_eq!(
        committed, generated,
        "committed query-schema.json is stale; regenerate with `UPDATE_QUERY_SCHEMA=1 cargo \
         test -p posthaste-domain-model --test query_schema_contract` and re-run \
         `bun run query-schema:generate` in apps/web"
    );
}
