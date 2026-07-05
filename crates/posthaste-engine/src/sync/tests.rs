use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::net::TcpListener;

use crate::live::connect_jmap_client;

use super::*;

#[test]
fn non_empty_state_rejects_empty_cursor() {
    assert_eq!(super::non_empty_state("cursor-1"), Some("cursor-1"));
    assert_eq!(super::non_empty_state(""), None);
}

#[test]
fn email_cursor_state_requires_current_metadata_version() {
    let encoded = super::encode_email_cursor_state("server-state-1");

    assert_eq!(
        super::decode_email_cursor_state(&encoded),
        Some("server-state-1".to_string())
    );
    assert_eq!(super::decode_email_cursor_state("server-state-1"), None);
    assert_eq!(
        super::decode_email_cursor_state(
            r#"{"kind":"jmap-email","metadataVersion":1,"state":"server-state-1"}"#,
        ),
        None
    );
}

#[test]
fn email_metadata_sync_requests_threading_headers_and_recipients() {
    let properties = super::email_metadata_properties();

    assert!(properties.contains(&email::Property::To));
    assert!(properties.contains(&email::Property::SentAt));
    assert!(properties.contains(&email::Property::MessageId));
    assert!(properties.contains(&email::Property::References));
    assert!(properties.contains(&email::Property::InReplyTo));
}

#[tokio::test]
async fn empty_email_cursor_recovers_via_full_sync_and_persists_real_state() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock JMAP server");
    let addr = listener.local_addr().expect("mock server addr");
    let app_state = Arc::new(MockJmapState {
        base_url: format!("http://{addr}"),
        seen_methods: Mutex::new(Vec::new()),
    });
    let app = Router::new()
        .route("/.well-known/jmap", get(mock_session))
        .route("/api", post(mock_api))
        .with_state(app_state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock JMAP");
    });

    let client = connect_jmap_client(&format!("http://{addr}"), Some("dev"), "devpass")
        .await
        .expect("connect mock client");

    let sync = fetch_email_sync(&client, Some(""))
        .await
        .expect("empty cursor should trigger full sync");

    assert!(sync.messages.is_empty());
    assert_eq!(
        super::decode_email_cursor_state(&sync.cursor.state),
        Some("email-state-1".to_string())
    );

    let seen_methods = app_state
        .seen_methods
        .lock()
        .expect("seen methods lock poisoned")
        .clone();
    assert_eq!(seen_methods, vec!["Email/query", "Email/get"]);

    server.abort();
    let _ = server.await;
}

struct MockJmapState {
    base_url: String,
    seen_methods: Mutex<Vec<String>>,
}

/// Records the chunks a streamed sync emits so the test can assert progressive
/// delivery (mailboxes first, then message pages) and per-chunk contents.
#[derive(Default)]
struct RecordingSink {
    chunks: Vec<posthaste_domain_model::SyncBatch>,
}

#[async_trait::async_trait]
impl posthaste_domain_service::SyncChunkSink for RecordingSink {
    async fn emit(
        &mut self,
        batch: posthaste_domain_model::SyncBatch,
    ) -> Result<(), posthaste_domain_model::GatewayError> {
        self.chunks.push(batch);
        Ok(())
    }
}

#[tokio::test]
async fn full_streamed_sync_emits_mailbox_then_message_chunks_and_a_reconciliation() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock JMAP server");
    let addr = listener.local_addr().expect("mock server addr");
    let app_state = Arc::new(MockJmapState {
        base_url: format!("http://{addr}"),
        seen_methods: Mutex::new(Vec::new()),
    });
    let app = Router::new()
        .route("/.well-known/jmap", get(mock_session))
        .route("/api", post(mock_full_sync_api))
        .with_state(app_state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock JMAP");
    });

    let client = connect_jmap_client(&format!("http://{addr}"), Some("dev"), "devpass")
        .await
        .expect("connect mock client");
    let client = Arc::new(client);

    let mut sink = RecordingSink::default();
    let outcome = crate::live_sync::sync_account_streamed(
        &client,
        &posthaste_domain_model::AccountId::from("acc1"),
        &[],
        None,
        &mut sink,
    )
    .await
    .expect("streamed sync succeeds");

    // Mailbox chunk first (so message rows never reference an un-upserted
    // mailbox), then the message page. Chunks are upsert-only: no pruning or
    // cursors ride along; those are withheld for the reconciliation pass.
    assert_eq!(sink.chunks.len(), 2);
    assert_eq!(sink.chunks[0].mailboxes.len(), 1);
    assert!(sink.chunks[0].messages.is_empty());
    assert!(!sink.chunks[0].replace_all_mailboxes);
    assert!(sink.chunks[0].cursors.is_empty());
    assert_eq!(sink.chunks[1].messages.len(), 2);
    assert!(sink.chunks[1].mailboxes.is_empty());
    assert!(!sink.chunks[1].replace_all_messages);
    assert!(sink.chunks[1].cursors.is_empty());

    // A full snapshot of both object types yields a reconciliation set carrying
    // the complete remote ids, both prune flags, and the withheld cursors.
    let reconciliation = outcome
        .reconciliation
        .expect("full snapshot reconciles in a final pass");
    assert!(reconciliation.prune_messages);
    assert!(reconciliation.prune_mailboxes);
    assert_eq!(reconciliation.remote_message_ids.len(), 2);
    assert_eq!(reconciliation.remote_mailbox_ids.len(), 1);
    assert_eq!(reconciliation.cursors.len(), 2);

    server.abort();
    let _ = server.await;
}

/// DS1 mail-loss test: the full-snapshot `Email/query` is CAPPED (the server
/// returns fewer ids than requested, in pages) but honors `position`, so the
/// paginated fetch must walk it to EXHAUSTION and retrieve ALL five ids — then
/// prune-by-absence is allowed (the set is provably complete).
#[tokio::test]
async fn full_snapshot_pages_a_capped_email_query_to_exhaustion_and_prunes() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock JMAP server");
    let addr = listener.local_addr().expect("mock server addr");
    let app_state = Arc::new(MockJmapState {
        base_url: format!("http://{addr}"),
        seen_methods: Mutex::new(Vec::new()),
    });
    let app = Router::new()
        .route("/.well-known/jmap", get(mock_session))
        .route("/api", post(mock_paginated_full_sync_api))
        .with_state(app_state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock JMAP");
    });

    let client = connect_jmap_client(&format!("http://{addr}"), Some("dev"), "devpass")
        .await
        .expect("connect mock client");
    let client = Arc::new(client);

    let mut sink = RecordingSink::default();
    let outcome = crate::live_sync::sync_account_streamed(
        &client,
        &posthaste_domain_model::AccountId::from("acc1"),
        &[],
        None,
        &mut sink,
    )
    .await
    .expect("streamed sync succeeds");

    // Every remote message was retrieved despite the per-page cap of 2.
    let message_chunk = sink
        .chunks
        .iter()
        .find(|chunk| !chunk.messages.is_empty())
        .expect("a message chunk was emitted");
    let upserted: std::collections::BTreeSet<_> = message_chunk
        .messages
        .iter()
        .map(|message| message.id.as_str().to_string())
        .collect();
    assert_eq!(
        upserted,
        ["m1", "m2", "m3", "m4", "m5"]
            .into_iter()
            .map(String::from)
            .collect::<std::collections::BTreeSet<_>>(),
        "all five ids across three capped pages must be fetched and upserted",
    );

    // At least three Email/query pages were issued (5 ids, cap 2 → 3 pages).
    let query_pages = app_state
        .seen_methods
        .lock()
        .expect("seen methods lock poisoned")
        .iter()
        .filter(|method| method.as_str() == "Email/query")
        .count();
    assert!(
        query_pages >= 3,
        "the capped query must be paged to exhaustion, got {query_pages} pages",
    );

    // The set is provably complete (reached total), so pruning is enabled.
    let reconciliation = outcome
        .reconciliation
        .expect("full snapshot reconciles in a final pass");
    assert!(
        reconciliation.prune_messages,
        "a provably-complete remote set drives prune-by-absence",
    );
    assert_eq!(reconciliation.remote_message_ids.len(), 5);

    server.abort();
    let _ = server.await;
}

/// DS1 mail-loss test: the server CAPS `Email/query` and does NOT honor
/// `position` (a single unpaginated page is all we can ever get) and reports no
/// `total`, so the remote id set canNOT be proven complete. The fetch must
/// still upsert what it retrieved but REFUSE prune-by-absence (`prune_messages`
/// is `false`), never deleting local mail against an incomplete set.
#[tokio::test]
async fn capped_email_query_that_cannot_be_paged_refuses_to_prune() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock JMAP server");
    let addr = listener.local_addr().expect("mock server addr");
    let app_state = Arc::new(MockJmapState {
        base_url: format!("http://{addr}"),
        seen_methods: Mutex::new(Vec::new()),
    });
    let app = Router::new()
        .route("/.well-known/jmap", get(mock_session))
        .route("/api", post(mock_capped_stuck_full_sync_api))
        .with_state(app_state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock JMAP");
    });

    let client = connect_jmap_client(&format!("http://{addr}"), Some("dev"), "devpass")
        .await
        .expect("connect mock client");
    let client = Arc::new(client);

    let mut sink = RecordingSink::default();
    let outcome = crate::live_sync::sync_account_streamed(
        &client,
        &posthaste_domain_model::AccountId::from("acc1"),
        &[],
        None,
        &mut sink,
    )
    .await
    .expect("streamed sync succeeds");

    // What arrived is still upserted (progressive delivery is preserved)...
    let message_chunk = sink
        .chunks
        .iter()
        .find(|chunk| !chunk.messages.is_empty())
        .expect("a message chunk was emitted");
    assert_eq!(message_chunk.messages.len(), 2);

    // ...but the set is NOT provably complete, so pruning is DISABLED: no local
    // message can be deleted against this capped, unpageable id set.
    let reconciliation = outcome
        .reconciliation
        .expect("full snapshot reconciles in a final pass");
    assert!(
        !reconciliation.prune_messages,
        "an unprovable/incomplete remote set MUST NOT drive prune-by-absence",
    );

    server.abort();
    let _ = server.await;
}

/// Shared mailbox half of the full-sync mock: one `inbox` mailbox via
/// `Mailbox/query` + `Mailbox/get`, recording the method for assertions.
fn mock_mailbox_full_sync_response(method: &str) -> Option<Json<Value>> {
    match method {
        "Mailbox/query" => Some(Json(json!({
            "methodResponses": [[
                "Mailbox/query",
                { "accountId": "acc1", "queryState": "mq-1", "position": 0, "ids": ["inbox"] },
                "s0"
            ]],
            "sessionState": "session-1"
        }))),
        "Mailbox/get" => Some(Json(json!({
            "methodResponses": [[
                "Mailbox/get",
                {
                    "accountId": "acc1",
                    "state": "mailbox-state-1",
                    "list": [{ "id": "inbox", "name": "Inbox", "role": "inbox",
                               "totalEmails": 5, "unreadEmails": 0 }],
                    "notFound": []
                },
                "s0"
            ]],
            "sessionState": "session-1"
        }))),
        _ => None,
    }
}

/// Build an `Email/get` response for exactly the requested ids.
fn mock_email_get_response(method_calls: &Value) -> Json<Value> {
    let ids = method_calls[0][1]["ids"]
        .as_array()
        .expect("ids array present");
    let list: Vec<Value> = ids
        .iter()
        .map(|id| {
            let id = id.as_str().expect("id is string");
            json!({
                "id": id,
                "threadId": format!("t-{id}"),
                "mailboxIds": { "inbox": true },
                "keywords": {},
                "subject": format!("Subject {id}"),
                "receivedAt": "2026-03-31T10:00:00Z",
                "size": 10
            })
        })
        .collect();
    Json(json!({
        "methodResponses": [[
            "Email/get",
            { "accountId": "acc1", "state": "email-state-1", "list": list, "notFound": [] },
            "s0"
        ]],
        "sessionState": "session-1"
    }))
}

/// Full-sync mock whose `Email/query` CAPS each page to two ids but honors
/// `position` and reports `total`, so the paginated fetch can walk it to
/// exhaustion. Five ids: `m1`..`m5`.
async fn mock_paginated_full_sync_api(
    State(state): State<Arc<MockJmapState>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let method_calls = body["methodCalls"]
        .as_array()
        .expect("methodCalls array present");
    let method = method_calls[0][0]
        .as_str()
        .expect("method name present")
        .to_string();
    state
        .seen_methods
        .lock()
        .expect("seen methods lock poisoned")
        .push(method.clone());

    if let Some(response) = mock_mailbox_full_sync_response(&method) {
        return response;
    }
    match method.as_str() {
        "Email/query" => {
            const ALL: [&str; 5] = ["m1", "m2", "m3", "m4", "m5"];
            const CAP: usize = 2;
            let position = method_calls[0][1]["position"].as_i64().unwrap_or(0);
            let start = position.max(0) as usize;
            let page: Vec<&str> = ALL.iter().skip(start).take(CAP).copied().collect();
            Json(json!({
                "methodResponses": [[
                    "Email/query",
                    {
                        "accountId": "acc1",
                        "queryState": "eq-1",
                        "canCalculateChanges": true,
                        "position": start as i64,
                        "total": ALL.len(),
                        "limit": CAP,
                        "ids": page
                    },
                    "s0"
                ]],
                "sessionState": "session-1"
            }))
        }
        "Email/get" => mock_email_get_response(&Value::Array(method_calls.clone())),
        other => panic!("unexpected mock JMAP method: {other}"),
    }
}

/// Full-sync mock whose `Email/query` is capped AND ignores `position` (always
/// the same first page) and reports no `total` — an id set that cannot be
/// proven complete, so prune-by-absence must be refused.
async fn mock_capped_stuck_full_sync_api(
    State(state): State<Arc<MockJmapState>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let method_calls = body["methodCalls"]
        .as_array()
        .expect("methodCalls array present");
    let method = method_calls[0][0]
        .as_str()
        .expect("method name present")
        .to_string();
    state
        .seen_methods
        .lock()
        .expect("seen methods lock poisoned")
        .push(method.clone());

    if let Some(response) = mock_mailbox_full_sync_response(&method) {
        return response;
    }
    match method.as_str() {
        // Always the same two ids regardless of `position`, and no `total`.
        "Email/query" => Json(json!({
            "methodResponses": [[
                "Email/query",
                {
                    "accountId": "acc1",
                    "queryState": "eq-1",
                    "canCalculateChanges": true,
                    "position": 0,
                    "limit": 2,
                    "ids": ["m1", "m2"]
                },
                "s0"
            ]],
            "sessionState": "session-1"
        })),
        "Email/get" => mock_email_get_response(&Value::Array(method_calls.clone())),
        other => panic!("unexpected mock JMAP method: {other}"),
    }
}

async fn mock_full_sync_api(
    State(state): State<Arc<MockJmapState>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let method_calls = body["methodCalls"]
        .as_array()
        .expect("methodCalls array present");
    let method = method_calls[0][0]
        .as_str()
        .expect("method name present")
        .to_string();
    state
        .seen_methods
        .lock()
        .expect("seen methods lock poisoned")
        .push(method.clone());

    match method.as_str() {
        "Mailbox/query" => Json(json!({
            "methodResponses": [[
                "Mailbox/query",
                { "accountId": "acc1", "queryState": "mq-1", "position": 0, "ids": ["inbox"] },
                "s0"
            ]],
            "sessionState": "session-1"
        })),
        "Mailbox/get" => Json(json!({
            "methodResponses": [[
                "Mailbox/get",
                {
                    "accountId": "acc1",
                    "state": "mailbox-state-1",
                    "list": [{ "id": "inbox", "name": "Inbox", "role": "inbox",
                               "totalEmails": 2, "unreadEmails": 0 }],
                    "notFound": []
                },
                "s0"
            ]],
            "sessionState": "session-1"
        })),
        "Email/query" => Json(json!({
            "methodResponses": [[
                "Email/query",
                {
                    "accountId": "acc1",
                    "queryState": "eq-1",
                    "canCalculateChanges": true,
                    "position": 0,
                    "ids": ["m1", "m2"]
                },
                "s0"
            ]],
            "sessionState": "session-1"
        })),
        "Email/get" => {
            let ids = method_calls[0][1]["ids"]
                .as_array()
                .expect("ids array present");
            let list: Vec<Value> = ids
                .iter()
                .map(|id| {
                    let id = id.as_str().expect("id is string");
                    json!({
                        "id": id,
                        "threadId": format!("t-{id}"),
                        "mailboxIds": { "inbox": true },
                        "keywords": {},
                        "subject": format!("Subject {id}"),
                        "receivedAt": "2026-03-31T10:00:00Z",
                        "size": 10
                    })
                })
                .collect();
            Json(json!({
                "methodResponses": [[
                    "Email/get",
                    { "accountId": "acc1", "state": "email-state-1", "list": list, "notFound": [] },
                    "s0"
                ]],
                "sessionState": "session-1"
            }))
        }
        other => panic!("unexpected mock JMAP method: {other}"),
    }
}

async fn mock_session(State(state): State<Arc<MockJmapState>>) -> Json<Value> {
    Json(json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {
                "maxSizeUpload": 50000000,
                "maxConcurrentUpload": 4,
                "maxSizeRequest": 5000000,
                "maxConcurrentRequests": 4,
                "maxCallsInRequest": 16,
                "maxObjectsInGet": 500,
                "maxObjectsInSet": 500,
                "collationAlgorithms": ["i;ascii-casemap"]
            },
            "urn:ietf:params:jmap:mail": {}
        },
        "accounts": {
            "acc1": {
                "name": "Dev",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": {
                    "urn:ietf:params:jmap:mail": {}
                }
            }
        },
        "primaryAccounts": {
            "urn:ietf:params:jmap:mail": "acc1"
        },
        "username": "dev",
        "apiUrl": format!("{}/api", state.base_url),
        "downloadUrl": format!("{}/download/{{accountId}}/{{blobId}}/{{name}}", state.base_url),
        "uploadUrl": format!("{}/upload/{{accountId}}", state.base_url),
        "eventSourceUrl": format!("{}/event", state.base_url),
        "state": "session-1"
    }))
}

async fn mock_api(State(state): State<Arc<MockJmapState>>, Json(body): Json<Value>) -> Json<Value> {
    let method_calls = body["methodCalls"]
        .as_array()
        .expect("methodCalls array present");
    let method = method_calls[0][0]
        .as_str()
        .expect("method name present")
        .to_string();
    state
        .seen_methods
        .lock()
        .expect("seen methods lock poisoned")
        .push(method.clone());

    match method.as_str() {
        "Email/query" => Json(json!({
            "methodResponses": [[
                "Email/query",
                {
                    "accountId": "acc1",
                    "queryState": "query-1",
                    "canCalculateChanges": true,
                    "position": 0,
                    "ids": []
                },
                "s0"
            ]],
            "sessionState": "session-1"
        })),
        "Email/get" => {
            let ids = method_calls[0][1]["ids"]
                .as_array()
                .expect("ids array present");
            assert!(
                ids.is_empty(),
                "empty full sync should request Email/get with no ids"
            );
            Json(json!({
                "methodResponses": [[
                    "Email/get",
                    {
                        "accountId": "acc1",
                        "state": "email-state-1",
                        "list": [],
                        "notFound": []
                    },
                    "s0"
                ]],
                "sessionState": "session-1"
            }))
        }
        other => panic!("unexpected mock JMAP method: {other}"),
    }
}

/// Mailbox full-sync mock whose `Mailbox/query` is capped AND ignores `position`
/// (always the same first page) and reports no `total` — an id set that cannot
/// be proven complete, so mailbox prune-by-absence must be refused (DP-C3).
async fn mock_capped_stuck_mailbox_query_api(
    State(state): State<Arc<MockJmapState>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let method_calls = body["methodCalls"]
        .as_array()
        .expect("methodCalls array present");
    let method = method_calls[0][0]
        .as_str()
        .expect("method name present")
        .to_string();
    state
        .seen_methods
        .lock()
        .expect("seen methods lock poisoned")
        .push(method.clone());

    match method.as_str() {
        // Always the same two ids regardless of `position`, capped to the applied
        // `limit`, and no `total`: unpageable, so completeness cannot be proven.
        "Mailbox/query" => Json(json!({
            "methodResponses": [[
                "Mailbox/query",
                {
                    "accountId": "acc1",
                    "queryState": "mq-1",
                    "position": 0,
                    "limit": 2,
                    "ids": ["inbox", "archive"]
                },
                "s0"
            ]],
            "sessionState": "session-1"
        })),
        "Mailbox/get" => {
            let ids = method_calls[0][1]["ids"].as_array().expect("ids present");
            let list: Vec<Value> = ids
                .iter()
                .map(|id| {
                    let id = id.as_str().expect("id string");
                    json!({ "id": id, "name": id, "role": Value::Null,
                            "totalEmails": 0, "unreadEmails": 0 })
                })
                .collect();
            Json(json!({
                "methodResponses": [[
                    "Mailbox/get",
                    { "accountId": "acc1", "state": "mailbox-state-1", "list": list,
                      "notFound": [] },
                    "s0"
                ]],
                "sessionState": "session-1"
            }))
        }
        other => panic!("unexpected mock JMAP method: {other}"),
    }
}

/// DP-C3 mail-loss test: a capped, unpageable `Mailbox/query` (no `total`) cannot
/// be proven exhaustive, so the full mailbox snapshot upserts what it got but
/// does NOT earn `replace_all_mailboxes` — pruning is disabled so a transiently-
/// capped listing can never cascade-delete every local mailbox.
#[tokio::test]
async fn capped_mailbox_query_that_cannot_be_paged_refuses_to_prune() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock JMAP server");
    let addr = listener.local_addr().expect("mock server addr");
    let app_state = Arc::new(MockJmapState {
        base_url: format!("http://{addr}"),
        seen_methods: Mutex::new(Vec::new()),
    });
    let app = Router::new()
        .route("/.well-known/jmap", get(mock_session))
        .route("/api", post(mock_capped_stuck_mailbox_query_api))
        .with_state(app_state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock JMAP");
    });

    let client = connect_jmap_client(&format!("http://{addr}"), Some("dev"), "devpass")
        .await
        .expect("connect mock client");

    let sync = fetch_mailbox_sync(&client, None)
        .await
        .expect("full mailbox sync succeeds");

    assert_eq!(sync.mailboxes.len(), 2, "what arrived is still upserted");
    assert!(
        !sync.replace_all_mailboxes,
        "an unprovable/incomplete mailbox listing MUST NOT drive prune-by-absence",
    );

    server.abort();
    let _ = server.await;
}

/// A complete (short-tail) `Mailbox/query` earns `replace_all_mailboxes`, so a
/// genuinely-deleted mailbox is still pruned.
#[tokio::test]
async fn complete_mailbox_query_earns_prune() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock JMAP server");
    let addr = listener.local_addr().expect("mock server addr");
    let app_state = Arc::new(MockJmapState {
        base_url: format!("http://{addr}"),
        seen_methods: Mutex::new(Vec::new()),
    });
    let app = Router::new()
        .route("/.well-known/jmap", get(mock_session))
        .route(
            "/api",
            post(
                |State(state): State<Arc<MockJmapState>>, Json(body): Json<Value>| async move {
                    let method_calls = body["methodCalls"].as_array().expect("methodCalls");
                    let method = method_calls[0][0].as_str().expect("method").to_string();
                    state
                        .seen_methods
                        .lock()
                        .expect("lock")
                        .push(method.clone());
                    mock_mailbox_full_sync_response(&method)
                        .unwrap_or_else(|| panic!("unexpected method {method}"))
                },
            ),
        )
        .with_state(app_state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock JMAP");
    });

    let client = connect_jmap_client(&format!("http://{addr}"), Some("dev"), "devpass")
        .await
        .expect("connect mock client");

    let sync = fetch_mailbox_sync(&client, None)
        .await
        .expect("full mailbox sync succeeds");

    assert_eq!(sync.mailboxes.len(), 1);
    assert!(
        sync.replace_all_mailboxes,
        "a provably-complete mailbox listing earns prune-by-absence",
    );

    server.abort();
    let _ = server.await;
}
