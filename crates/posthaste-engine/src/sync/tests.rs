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
