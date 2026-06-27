//! WASM helpers for message-mutation parsing and diff arithmetic.
//!
//! The web client used to maintain a hand-written TypeScript copy of the
//! name→assertion mapping and the `MessageChangeDiff` inverse. These functions
//! expose the canonical Rust implementations from `posthaste-link-contract`
//! and `posthaste-link-core` across the WASM boundary, eliminating that drift.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use std::collections::HashMap;

use posthaste_link_contract::message_mutation::MessageMutation;
use posthaste_link_core::{MessageAssertion, MessageChangeDiff};
use posthaste_runtime_contract::MutationRequest;

/// Parse a runtime mutation request and return `{ messageId, assertion }` as
/// JSON when the mutation is locally foldable. Returns `null` for mutations
/// whose effect cannot be folded from the request alone. `role_map_json` is the
/// account's role→mailbox-id map (`{"archive": "mbx-..."}`, built client-side
/// from the mailbox list); it resolves role moves (archive/trash/restoreToInbox/
/// moveToRole) to `ReplaceMailboxes`. `{}` → role moves get no optimism (graceful
/// when the mailbox list isn't loaded yet). Mirrors the Rust near-node
/// `MessageMutation::from_request` + `to_assertion_with_roles` path.
#[wasm_bindgen(js_name = parseMessageMutation)]
pub fn parse_message_mutation(
    request_json: &str,
    role_map_json: &str,
) -> Result<Option<String>, JsError> {
    let request: MutationRequest =
        serde_json::from_str(request_json).map_err(|error| JsError::new(&error.to_string()))?;
    let roles: HashMap<String, String> =
        serde_json::from_str(role_map_json).map_err(|error| JsError::new(&error.to_string()))?;
    // Unknown or non-foldable message mutations are not errors; the adapter
    // simply passes them through to the runtime.
    let mutation = match MessageMutation::from_request(&request) {
        Ok(mutation) => mutation,
        Err(_) => return Ok(None),
    };
    let assertion = match mutation.to_assertion_with_roles(&roles) {
        Some(assertion) => assertion,
        None => return Ok(None),
    };
    let output = ParsedMutation {
        message_id: mutation.message_id().to_string(),
        assertion,
    };
    serde_json::to_string(&output)
        .map(Some)
        .map_err(|error| JsError::new(&error.to_string()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ParsedMutation {
    message_id: String,
    assertion: MessageAssertion,
}

/// Swap added↔removed for both the keyword and mailbox facets — the inverse
/// diff applied by undo. Uses `MessageChangeDiff::inverse` in Rust.
#[wasm_bindgen(js_name = invertMessageChangeDiff)]
pub fn invert_message_change_diff(diff_json: &str) -> Result<String, JsError> {
    let diff: MessageChangeDiff =
        serde_json::from_str(diff_json).map_err(|error| JsError::new(&error.to_string()))?;
    let inverted = diff.inverse();
    serde_json::to_string(&inverted).map_err(|error| JsError::new(&error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_set_keywords_mutation() {
        let request = json!({
            "name": "message.setKeywords",
            "args": {
                "sourceId": "acct-1",
                "messageId": "msg-1",
                "command": {
                    "add": ["$flagged"],
                    "remove": []
                }
            },
            "clientMutationId": "op-1"
        });
        let output = parse_message_mutation(&request.to_string(), "{}")
            .unwrap()
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["messageId"], "msg-1");
        assert_eq!(parsed["assertion"]["kind"], "setKeywords");
        assert_eq!(parsed["assertion"]["add"], json!(["$flagged"]));
    }

    #[test]
    fn parse_set_read_state_mutation() {
        let request = json!({
            "name": "message.setReadState",
            "args": {
                "sourceId": "acct-1",
                "messageId": "msg-1",
                "read": true
            },
            "clientMutationId": "op-1"
        });
        let output = parse_message_mutation(&request.to_string(), "{}")
            .unwrap()
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["assertion"]["kind"], "setKeywords");
        assert!(parsed["assertion"]["add"]
            .as_array()
            .unwrap()
            .contains(&json!("$seen")));
    }

    #[test]
    fn parse_archive_resolves_via_the_role_map() {
        let request = json!({
            "name": "message.archive",
            "args": {
                "sourceId": "acct-1",
                "messageId": "msg-1"
            },
            "clientMutationId": "op-1"
        });
        // No role map → no optimism (graceful: mailbox list not loaded yet).
        assert!(parse_message_mutation(&request.to_string(), "{}")
            .unwrap()
            .is_none());
        // With the account's archive mailbox → ReplaceMailboxes.
        let output =
            parse_message_mutation(&request.to_string(), r#"{"archive":"mbx-a"}"#)
                .unwrap()
                .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["assertion"]["kind"], "replaceMailboxes");
        assert_eq!(parsed["assertion"]["mailbox_ids"], json!(["mbx-a"]));
    }

    #[test]
    fn invert_message_change_diff_swaps_facets() {
        let diff = json!({
            "keywords": { "added": ["$flagged"], "removed": ["$seen"] },
            "mailboxes": { "added": ["archive"], "removed": ["inbox"] }
        });
        let output = invert_message_change_diff(&diff.to_string()).unwrap();
        let inverted: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(inverted["keywords"]["added"], json!(["$seen"]));
        assert_eq!(inverted["keywords"]["removed"], json!(["$flagged"]));
        assert_eq!(inverted["mailboxes"]["added"], json!(["inbox"]));
        assert_eq!(inverted["mailboxes"]["removed"], json!(["archive"]));
    }
}
