//! Per-operation authorization for the named-mutation funnel
//! (`POST /v1/runtime/sessions/{id}/mutations`), driven through the REAL
//! production router (`build_api_router`) — real perimeter middleware AND the
//! real handler whose per-op derivation is under test, not stubs.
//!
//! The route used to be statically `Tag`-gated, which both UNDER-gated (a
//! tag-scoped token could submit `message.destroy` or `message.send`) and
//! OVER-blocked (a move-scoped token could not archive; the `{mint, read}`
//! discovery bootstrap 403'd every mutation, breaking the web e2e scripts).
//! These cases pin the fix from the wire: the action is derived per operation
//! in the handler, deny-by-default.
//!
//! @spec docs/eph/DESIGN-L1-capability-tokens

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use posthaste_http_api_adapter::build_api_router;
use posthaste_http_api_adapter::token::{attenuate, mint_full_scope_token, mint_with_caveats};
use serde_json::{json, Value};
use tower::{Service, ServiceExt};

use super::support::{build_state, test_root_key};

/// The real `/v1` app (production router + auth perimeter), one shared state.
fn real_app() -> Router {
    Router::new().nest("/v1", build_api_router(build_state()))
}

async fn call(
    app: &mut Router,
    method: &str,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "127.0.0.1")
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let response = ServiceExt::<Request<Body>>::ready(app)
        .await
        .expect("router ready")
        .call(builder.body(body).expect("request builds"))
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body collects")
        .to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

/// Open a runtime link with the full-scope token; returns its id.
async fn open_link(app: &mut Router, full_scope: &str) -> String {
    let (status, body) = call(app, "POST", "/v1/runtime/sessions", full_scope, None).await;
    assert_eq!(status, StatusCode::OK, "open link: {body}");
    body["linkId"].as_str().expect("linkId present").to_string()
}

/// Open an `acct-a`-scoped runtime link (the runtime pins a link to its
/// caller's account scope, so account-scoped callers need a matching link).
async fn open_scoped_link(app: &mut Router, full_scope: &str) -> String {
    let (status, body) = call(
        app,
        "POST",
        "/v1/runtime/sessions?sourceId=acct-a",
        full_scope,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "open scoped link: {body}");
    body["linkId"].as_str().expect("linkId present").to_string()
}

fn mutation_path(link: &str) -> String {
    format!("/v1/runtime/sessions/{link}/mutations")
}

/// Mutation path with the account query filter an account-caveated token needs
/// to satisfy the perimeter's `Filter` rule (exactly what the web client sends).
fn scoped_mutation_path(link: &str) -> String {
    format!("/v1/runtime/sessions/{link}/mutations?sourceId=acct-a")
}

/// A benign, self-contained mutation: the `revCursor` control op (no message
/// target, idempotent cursor assignment) — it settles 200 end-to-end without
/// needing a provisioned account/message fixture.
fn rev_cursor(id: &str) -> Value {
    json!({
        "name": "revCursor",
        "args": { "accountId": "acct-a", "cursorStepId": null, "redoTail": [] },
        "clientMutationId": id
    })
}

fn set_keywords(id: &str) -> Value {
    json!({
        "name": "message.setKeywords",
        "args": { "sourceId": "acct-a", "messageId": "m1",
                  "command": { "add": ["$seen"], "remove": [] } },
        "clientMutationId": id
    })
}

fn destroy(id: &str) -> Value {
    json!({
        "name": "message.destroy",
        "args": { "sourceId": "acct-a", "messageId": "m1" },
        "clientMutationId": id
    })
}

/// The dev-stack discovery bootstrap: the full-scope credential attenuated to
/// exactly `{mint, read}` — the token `daemon.json` carries and the web e2e
/// scripts read. Built with the SAME attenuation the server applies.
fn bootstrap_capability(full_scope: &str) -> String {
    attenuate(full_scope, "action = mint,read").expect("bootstrap attenuates")
}

/// Full-scope (the embedded webview token): a benign mutation settles 200
/// end-to-end through the real handler and runtime.
#[tokio::test]
async fn full_scope_token_runs_a_benign_mutation() {
    let mut app = real_app();
    let full = mint_full_scope_token(&test_root_key());
    let link = open_link(&mut app, &full).await;
    let (status, body) = call(
        &mut app,
        "POST",
        &mutation_path(&link),
        &full,
        Some(rev_cursor("cm-full-1")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "receipt: {body}");
}

/// The security half (the hole): a tag-scoped token can no longer submit
/// destructive or send operations through the funnel — and the deny is the
/// HANDLER's per-op 403, i.e. the same token passes the perimeter and runs a
/// tag-class operation without an authz denial.
#[tokio::test]
async fn tag_scoped_token_cannot_destroy_or_send() {
    let mut app = real_app();
    let full = mint_full_scope_token(&test_root_key());
    let link = open_scoped_link(&mut app, &full).await;
    let tag = mint_with_caveats(&test_root_key(), &["action = tag", "account = acct-a"]);

    let (status, body) = call(
        &mut app,
        "POST",
        &scoped_mutation_path(&link),
        &tag,
        Some(destroy("cm-tag-1")),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "destroy must 403: {body}");

    let (status, body) = call(
        &mut app,
        "POST",
        &scoped_mutation_path(&link),
        &tag,
        Some(json!({
            "name": "message.send",
            "args": { "sourceId": "acct-a", "messageId": "d1",
                      "request": { "from": null, "to": [], "cc": [], "bcc": [],
                                   "subject": "s", "body": "b",
                                   "inReplyTo": null, "references": null,
                                   "draftId": "d1" } },
            "clientMutationId": "cm-tag-2"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "send must 403: {body}");

    // Same token, tag-class op: passes authz (any failure past that point
    // would be a domain error about the unprovisioned account, never a 401/403).
    let (status, body) = call(
        &mut app,
        "POST",
        &scoped_mutation_path(&link),
        &tag,
        Some(set_keywords("cm-tag-3")),
    )
    .await;
    assert_ne!(status, StatusCode::FORBIDDEN, "setKeywords authz: {body}");
    assert_ne!(
        status,
        StatusCode::UNAUTHORIZED,
        "setKeywords authz: {body}"
    );
}

/// The over-block half: a move-scoped token can archive (replaceMailboxes)
/// through the funnel — the old static Tag gate denied this at the perimeter —
/// while tag-class operations stay walled off.
#[tokio::test]
async fn move_scoped_token_can_move_but_not_tag() {
    let mut app = real_app();
    let full = mint_full_scope_token(&test_root_key());
    let link = open_scoped_link(&mut app, &full).await;
    let mover = mint_with_caveats(&test_root_key(), &["action = move", "account = acct-a"]);

    let (status, body) = call(
        &mut app,
        "POST",
        &scoped_mutation_path(&link),
        &mover,
        Some(json!({
            "name": "message.replaceMailboxes",
            "args": { "sourceId": "acct-a", "messageId": "m1", "mailboxIds": ["mbx-archive"] },
            "clientMutationId": "cm-move-1"
        })),
    )
    .await;
    assert_ne!(status, StatusCode::FORBIDDEN, "move authz: {body}");
    assert_ne!(status, StatusCode::UNAUTHORIZED, "move authz: {body}");

    let (status, body) = call(
        &mut app,
        "POST",
        &scoped_mutation_path(&link),
        &mover,
        Some(set_keywords("cm-move-2")),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "setKeywords must 403: {body}"
    );
}

/// The e2e regression, exactly the dev-stack flow the web e2e scripts now use:
/// the `{mint, read}` bootstrap from `daemon.json` cannot mutate directly (it
/// holds no write verb — deny-by-default), so the script MINTS a write-capable
/// session token via `POST /v1/auth/tokens` (the bootstrap's `mint` right) and
/// submits the mutation with THAT → 200.
#[tokio::test]
async fn bootstrap_token_mints_a_write_token_that_mutates() {
    let mut app = real_app();
    let full = mint_full_scope_token(&test_root_key());
    let link = open_link(&mut app, &full).await;
    let bootstrap = bootstrap_capability(&full);

    // Direct mutation with the bootstrap: 403 (this is what silently broke the
    // e2e scripts under the old static gate, and stays a deliberate wall).
    let (status, body) = call(
        &mut app,
        "POST",
        &mutation_path(&link),
        &bootstrap,
        Some(rev_cursor("cm-boot-1")),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "bootstrap direct: {body}");

    // The fixed flow: mint a scoped, expiring write token from the bootstrap…
    let (status, body) = call(
        &mut app,
        "POST",
        "/v1/auth/tokens",
        &bootstrap,
        Some(json!({
            "actions": ["read", "tag", "move", "delete", "send"],
            "expiresInSeconds": 3600
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "mint: {body}");
    let session_token = body["token"].as_str().expect("minted token").to_string();

    // …and the benign mutation goes through, end to end.
    let (status, body) = call(
        &mut app,
        "POST",
        &mutation_path(&link),
        &session_token,
        Some(rev_cursor("cm-boot-2")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "minted-token mutation: {body}");
}

/// A read-only token is denied EVERY operation through the funnel, including
/// the revCursor control op (undo/redo bookkeeping is a write).
#[tokio::test]
async fn read_only_token_is_denied_all_mutations() {
    let mut app = real_app();
    let full = mint_full_scope_token(&test_root_key());
    let link = open_link(&mut app, &full).await;
    let read_only = mint_with_caveats(&test_root_key(), &["action = read"]);
    for (label, body) in [
        ("revCursor", rev_cursor("cm-ro-1")),
        ("setKeywords", set_keywords("cm-ro-2")),
        ("destroy", destroy("cm-ro-3")),
    ] {
        let (status, response) = call(
            &mut app,
            "POST",
            &mutation_path(&link),
            &read_only,
            Some(body),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{label}: {response}");
    }
}
