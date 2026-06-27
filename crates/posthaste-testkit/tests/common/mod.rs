//! Shared helpers for `posthaste-testkit` integration tests.
//!
//! Each file in `tests/` is its own test binary, so shared helpers live here
//! and are pulled in via `#[path = "common/mod.rs"] mod common;`.

use posthaste_domain::{MessageSortField, SortDirection};
use posthaste_runtime_contract::{MailPresentationRequest, MailQueryRequest, ViewDescriptor};

/// A `mailList` view descriptor for `query`: messages projection, newest-first,
/// page of 50. Shared by the view-settlement, live-convergence, and
/// fixture-loader tests.
pub fn mail_list_view(query: &str) -> ViewDescriptor {
    let request = MailQueryRequest {
        query: query.to_string(),
        presentation: MailPresentationRequest::Messages {
            limit: Some(50),
            cursor: None,
            sort_field: MessageSortField::Date,
            sort_direction: SortDirection::Desc,
        },
        visibility: None,
    };
    ViewDescriptor {
        family: "mailList".to_string(),
        payload: serde_json::to_value(&request).expect("request should serialize"),
        // An evaluable `date`-sorted source-mailbox view — the client
        // self-maintains it, so the runtime skips the per-event re-serve
        // (option iii). Tests asserting `not_recomputed` rely on this.
        client_self_maintained: true,
    }
}
