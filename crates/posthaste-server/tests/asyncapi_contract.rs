//! Contract test: the committed `asyncapi.json` event contract must stay in
//! sync with the topics the backend actually emits. The documented
//! `components.schemas.EventTopic` enum is compared, both directions, against
//! `posthaste_domain_service::ALL_EVENT_TOPICS` (the single source of truth). Drift here
//! means the published event contract has fallen out of step with the server.
//!
//! @spec docs/L1-api#sse-event-stream

use std::collections::BTreeSet;
use std::path::PathBuf;

/// Repo-root `asyncapi.json` — the committed event contract artifact.
fn committed_asyncapi_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("asyncapi.json")
}

/// The `topic` enum documented in the committed `asyncapi.json`.
fn documented_topics() -> BTreeSet<String> {
    let path = committed_asyncapi_path();
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let doc: serde_json::Value =
        serde_json::from_str(&raw).expect("asyncapi.json should be valid JSON");

    let enum_values = doc
        .pointer("/components/schemas/EventTopic/enum")
        .and_then(|v| v.as_array())
        .expect("asyncapi.json should define components.schemas.EventTopic.enum as an array");

    enum_values
        .iter()
        .map(|v| {
            v.as_str()
                .expect("each EventTopic enum value should be a string")
                .to_string()
        })
        .collect()
}

#[test]
fn asyncapi_topics_match_backend_constants() {
    let documented = documented_topics();
    let constants: BTreeSet<String> = posthaste_domain_service::ALL_EVENT_TOPICS
        .iter()
        .map(|s| s.to_string())
        .collect();

    let missing_from_doc: Vec<&String> = constants.difference(&documented).collect();
    let missing_from_constants: Vec<&String> = documented.difference(&constants).collect();

    assert!(
        missing_from_doc.is_empty(),
        "topics emitted by the backend are missing from asyncapi.json EventTopic enum: {missing_from_doc:?}"
    );
    assert!(
        missing_from_constants.is_empty(),
        "topics documented in asyncapi.json are not in posthaste_domain_service::ALL_EVENT_TOPICS: {missing_from_constants:?}"
    );

    assert_eq!(
        documented, constants,
        "asyncapi.json EventTopic enum has drifted from posthaste_domain_service::ALL_EVENT_TOPICS"
    );
}
