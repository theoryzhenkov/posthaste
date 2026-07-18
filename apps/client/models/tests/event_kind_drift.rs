//! Pins [`DomainEventKind`] to the emitted topic set: every variant must
//! serialize to its `posthaste_domain_model::ALL_EVENT_TOPICS` entry, in
//! declaration order, and vice versa — an added, removed, or renamed topic
//! on either side fails here.

use posthaste_client_models::DomainEventKind;
use posthaste_domain_model::ALL_EVENT_TOPICS;

#[test]
fn kind_variants_match_all_event_topics_exactly() {
    assert_eq!(
        DomainEventKind::ALL.len(),
        ALL_EVENT_TOPICS.len(),
        "DomainEventKind and ALL_EVENT_TOPICS disagree on the topic count"
    );
    for (kind, topic) in DomainEventKind::ALL.iter().zip(ALL_EVENT_TOPICS) {
        let serialized = serde_json::to_value(kind).unwrap();
        assert_eq!(
            serialized,
            serde_json::Value::String((*topic).to_string()),
            "variant {kind:?} does not serialize to its topic"
        );
        let parsed: DomainEventKind = serde_json::from_value(serialized).unwrap();
        assert_eq!(parsed, *kind);
    }
}
