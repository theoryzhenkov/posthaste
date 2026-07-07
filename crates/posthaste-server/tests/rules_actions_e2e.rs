//! The release rule-action vocabulary, end to end on the exact bundled-server
//! assembly (`posthaste_server::start_server`), in its own test binary (the
//! one-server-boot-per-process rule — see `rule_fired_support`).
//!
//! Pins, in one server lifetime:
//!
//! 1. **Boundary validation** (the shared `validate_rule_action` gate at the
//!    REST write surface): an unconditional `destroy` and a `moveToRole` onto a
//!    non-assignable role are 400'd; the assignable forms are 201'd; and
//!    `stopProcessing` round-trips through create → list.
//! 2. **Chaining semantics**: a matched rule with `stopProcessing = true`
//!    short-circuits later rules for that fact (the later rule neither fires a
//!    `rule.fired` nor applies its tag).
//! 3. **Executor dispatch through the live engine** for the new Level-0
//!    actions: `markRead` flips `isRead`, `flag` flips `isFlagged`.
//! 4. **Destroy's destructive path**: a `destroy` rule permanently removes the
//!    matched message — and ONLY the matched message — through the existing
//!    `message.destroy` machinery (no new deletion path).

mod rule_fired_support;

use rule_fired_support::Harness;
use serde_json::{json, Value};

/// A WHEN-clause tree matching `keyword equals <value>`.
fn when_keyword(value: &str) -> Value {
    json!({
        "root": {
            "operator": "all",
            "negated": false,
            "nodes": [
                { "type": "condition", "field": "keyword", "operator": "equals",
                  "negated": false, "value": value }
            ]
        }
    })
}

/// An unconditional (condition-free) WHEN-clause tree.
fn when_empty() -> Value {
    json!({ "root": { "operator": "all", "negated": false, "nodes": [] } })
}

fn message_by_id<'a>(listing: &'a Value, id: &str) -> Option<&'a Value> {
    listing["items"]
        .as_array()?
        .iter()
        .find(|item| item["id"].as_str() == Some(id))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn release_action_vocabulary_boundary_chaining_executors_and_destroy() {
    // Boot with NO authored rules; everything is created over REST.
    let harness = Harness::start("rules-actions", "").await;

    // --- 1. Boundary validation ------------------------------------------------
    let (status, body) = harness
        .post(
            "/rules",
            json!({
                "name": "wipe everything",
                "when": when_empty(),
                "action": { "kind": "destroy" }
            }),
        )
        .await;
    assert_eq!(
        status,
        reqwest::StatusCode::BAD_REQUEST,
        "an unconditional destroy must be refused at the write boundary: {body}"
    );

    let (status, body) = harness
        .post(
            "/rules",
            json!({
                "name": "sneaky snooze",
                "when": when_keyword("x"),
                "action": { "kind": "moveToRole", "role": "snooze" }
            }),
        )
        .await;
    assert_eq!(
        status,
        reqwest::StatusCode::BAD_REQUEST,
        "moveToRole onto a non-assignable role must be refused: {body}"
    );

    // The assignable role form is accepted — created disabled so it stays a
    // boundary-only probe (the mock account need not have role mailboxes).
    let (status, body) = harness
        .post(
            "/rules",
            json!({
                "id": "probe-archive",
                "name": "0 archive probe",
                "when": when_keyword("never-tagged"),
                "action": { "kind": "moveToRole", "role": "archive" },
                "enabled": false
            }),
        )
        .await;
    assert_eq!(
        status,
        reqwest::StatusCode::CREATED,
        "moveToRole archive must be creatable: {body}"
    );

    // --- Live rules for the chaining / executor / destroy scenarios ------------
    // Managed rules evaluate sorted by name; the letter prefixes fix the order.
    for (id, rule) in [
        (
            "d-destroy",
            json!({
                "id": "d-destroy",
                "name": "D destroy purge",
                "when": when_keyword("purge"),
                "action": { "kind": "destroy" }
            }),
        ),
        (
            "f-flag",
            json!({
                "id": "f-flag",
                "name": "F flag",
                "when": when_keyword("flag-me"),
                "action": { "kind": "flag", "flagged": true }
            }),
        ),
        (
            "m-read",
            json!({
                "id": "m-read",
                "name": "M mark read",
                "when": when_keyword("read-me"),
                "action": { "kind": "markRead", "read": true }
            }),
        ),
        (
            "t-tag",
            json!({
                "id": "t-tag",
                "name": "T tag receipt",
                "when": when_keyword("tag-me"),
                "action": { "kind": "tag", "tag": "receipt" }
            }),
        ),
        (
            "y-stop",
            json!({
                "id": "y-stop",
                "name": "Y emit and stop",
                "when": when_keyword("instruct"),
                "action": { "kind": "emit" },
                "stopProcessing": true
            }),
        ),
        (
            "z-late",
            json!({
                "id": "z-late",
                "name": "Z tag after stop",
                "when": when_keyword("instruct"),
                "action": { "kind": "tag", "tag": "second-tag" }
            }),
        ),
    ] {
        let (status, body) = harness.post("/rules", rule).await;
        assert_eq!(
            status,
            reqwest::StatusCode::CREATED,
            "creating {id} should 201: {body}"
        );
    }

    // `stopProcessing` round-trips onto the read surface.
    let listing = harness.get("/rules").await;
    let stop_rule = listing["rules"]
        .as_array()
        .expect("rules array")
        .iter()
        .find(|rule| rule["id"].as_str() == Some("y-stop"))
        .expect("y-stop listed");
    assert_eq!(
        stop_rule["stopProcessing"].as_bool(),
        Some(true),
        "stopProcessing must round-trip: {stop_rule}"
    );

    // --- Seed one mock account ------------------------------------------------
    let account_id = "actions-acct";
    let message_id = harness.seed_account(account_id).await;

    // --- 2. Chaining: Y (stopProcessing) fires, Z never runs -------------------
    harness.tag(account_id, &message_id, "instruct").await;
    let sse = harness
        .events_containing(0, &["\"ruleId\":\"y-stop\""])
        .await;
    assert!(
        !sse.contains("\"ruleId\":\"z-late\""),
        "a rule after a stopProcessing match must not fire: {sse}"
    );
    // Grace period, then re-check durable history AND the message itself.
    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    let listing = harness.list_messages(account_id).await;
    let message = message_by_id(&listing, &message_id).expect("message present");
    let keywords = message["keywords"]
        .as_array()
        .map(|k| k.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    assert!(
        !keywords.contains(&"second-tag"),
        "the post-stop rule's tag must never land: {keywords:?}"
    );

    // --- 3. Executor dispatch: markRead and flag --------------------------------
    harness.tag(account_id, &message_id, "read-me").await;
    harness
        .wait_for_messages(account_id, "the markRead rule flips isRead", |listing| {
            message_by_id(listing, &message_id)
                .map(|m| m["isRead"].as_bool() == Some(true))
                .unwrap_or(false)
        })
        .await;

    // The tag action itself: regression pin for the SetUserTags → SetKeywords
    // bridge projection (a tag rule used to fail silently at the direct-apply
    // boundary as a "replica-only" operation).
    harness.tag(account_id, &message_id, "tag-me").await;
    harness
        .wait_for_messages(account_id, "the tag rule applies its tag", |listing| {
            message_by_id(listing, &message_id)
                .and_then(|m| m["keywords"].as_array())
                .map(|keywords| keywords.iter().any(|k| k.as_str() == Some("receipt")))
                .unwrap_or(false)
        })
        .await;

    harness.tag(account_id, &message_id, "flag-me").await;
    harness
        .wait_for_messages(account_id, "the flag rule flips isFlagged", |listing| {
            message_by_id(listing, &message_id)
                .map(|m| m["isFlagged"].as_bool() == Some(true))
                .unwrap_or(false)
        })
        .await;

    // --- 4. Destroy's destructive path ------------------------------------------
    // Count survivors first, so we can assert ONLY the matched message vanishes.
    let before = harness.list_messages(account_id).await;
    let count_before = before["items"].as_array().map(Vec::len).unwrap_or(0);
    assert!(count_before >= 1, "seeded account has messages");

    harness.tag(account_id, &message_id, "purge").await;
    let listing = harness
        .wait_for_messages(
            account_id,
            "the destroy rule permanently removes the matched message",
            |listing| message_by_id(listing, &message_id).is_none(),
        )
        .await;
    let count_after = listing["items"].as_array().map(Vec::len).unwrap_or(0);
    assert_eq!(
        count_after,
        count_before - 1,
        "exactly the matched message is destroyed — no collateral: {listing}"
    );
    // The destroy is observable as a fact on the tap, like every action.
    harness
        .events_containing(
            0,
            &["\"ruleId\":\"d-destroy\"", "\"actionKind\":\"destroy\""],
        )
        .await;

    harness.shutdown().await;
}
