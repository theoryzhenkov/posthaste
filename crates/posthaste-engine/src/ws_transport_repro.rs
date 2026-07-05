//! M67 reproduction: multi-method JMAP requests over the shared WS push
//! connection hang while single-method requests complete.
//!
//! These tests stand up a minimal in-process JMAP server that speaks the
//! WebSocket transport (session discovery over HTTP + a raw WS endpoint) so we
//! can exercise `LiveJmapGateway::send_request` over a real `CorrelatedWs`
//! socket, bounded by short timeouts so a regression fails fast instead of
//! hanging CI.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

use crate::live::{connect_jmap_client, LiveJmapGateway};

/// How the mock WS endpoint should behave, per scenario.
#[derive(Clone, Copy)]
struct WsBehavior {
    /// Number of unsolicited push notifications to emit immediately on connect,
    /// before servicing any request. Used to saturate the fork's bounded push
    /// channel when the consumer is not draining.
    flood_pushes: usize,
}

struct MockState {
    ws_url: String,
    base_url: String,
}

async fn mock_session(State(state): State<Arc<MockState>>) -> Json<Value> {
    Json(json!({
        "capabilities": {
            "urn:ietf:params:jmap:core": {
                "maxSizeUpload": 50000000u64,
                "maxConcurrentUpload": 4,
                "maxSizeRequest": 5000000u64,
                "maxConcurrentRequests": 4,
                "maxCallsInRequest": 16,
                "maxObjectsInGet": 500,
                "maxObjectsInSet": 500,
                "collationAlgorithms": ["i;ascii-casemap"]
            },
            "urn:ietf:params:jmap:mail": {},
            "urn:ietf:params:jmap:websocket": {
                "url": state.ws_url,
                "supportsPush": true
            }
        },
        "accounts": {
            "acc1": {
                "name": "Dev",
                "isPersonal": true,
                "isReadOnly": false,
                "accountCapabilities": { "urn:ietf:params:jmap:mail": {} }
            }
        },
        "primaryAccounts": { "urn:ietf:params:jmap:mail": "acc1" },
        "username": "dev",
        "apiUrl": format!("{}/api", state.base_url),
        "downloadUrl": format!("{}/download/{{accountId}}/{{blobId}}/{{name}}", state.base_url),
        "uploadUrl": format!("{}/upload/{{accountId}}", state.base_url),
        "eventSourceUrl": format!("{}/event", state.base_url),
        "state": "session-1"
    }))
}

/// A single StateChange push notification frame.
fn push_frame() -> Message {
    Message::text(
        json!({
            "@type": "StateChange",
            "changed": { "acc1": { "Email": "state-x" } }
        })
        .to_string(),
    )
}

/// Build a JMAP WS response for a request, echoing its `requestId` and emitting
/// one methodResponse per methodCall with a minimally-valid body.
fn response_for(request: &Value) -> Message {
    let request_id = request.get("id").and_then(Value::as_str).unwrap_or("0");
    let empty: Vec<Value> = Vec::new();
    let calls = request
        .get("methodCalls")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let method_responses: Vec<Value> = calls
        .iter()
        .map(|call| {
            let name = call.get(0).and_then(Value::as_str).unwrap_or("Core/echo");
            let call_id = call.get(2).and_then(Value::as_str).unwrap_or("c0");
            let body = match name {
                "Mailbox/get" => json!({
                    "accountId": "acc1",
                    "state": "mb-1",
                    "list": [{ "id": "Mb1", "name": "Drafts", "role": "drafts",
                               "sortOrder": 0, "totalEmails": 0, "unreadEmails": 0,
                               "totalThreads": 0, "unreadThreads": 0,
                               "myRights": {"mayReadItems": true, "mayAddItems": true,
                                            "mayRemoveItems": true, "maySetSeen": true,
                                            "maySetKeywords": true, "mayCreateChild": true,
                                            "mayRename": true, "mayDelete": true,
                                            "maySubmit": true},
                               "isSubscribed": true }],
                    "notFound": []
                }),
                "Identity/get" => json!({
                    "accountId": "acc1",
                    "state": "id-1",
                    "list": [],
                    "notFound": []
                }),
                _ => json!({
                    "accountId": "acc1",
                    "oldState": null,
                    "newState": "set-1",
                    "created": { "c0": { "id": "E1" } },
                    "updated": null,
                    "destroyed": null,
                    "notCreated": null,
                    "notUpdated": null,
                    "notDestroyed": null
                }),
            };
            json!([name, body, call_id])
        })
        .collect();

    Message::text(
        json!({
            "@type": "Response",
            "requestId": request_id,
            "methodResponses": method_responses,
            "sessionState": "session-1"
        })
        .to_string(),
    )
}

/// Spawn the mock JMAP server (HTTP session discovery + WS endpoint). Returns
/// the base HTTP URL the gateway should connect to.
async fn spawn_mock(behavior: WsBehavior) -> String {
    // WS endpoint: raw TCP + tungstenite accept.
    let ws_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind ws");
    let ws_addr: SocketAddr = ws_listener.local_addr().expect("ws addr");
    let ws_url = format!("ws://{ws_addr}/ws");

    tokio::spawn(async move {
        while let Ok((stream, _)) = ws_listener.accept().await {
            tokio::spawn(handle_ws(stream, behavior));
        }
    });

    // HTTP endpoint: session discovery.
    let http_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind http");
    let http_addr: SocketAddr = http_listener.local_addr().expect("http addr");
    let base_url = format!("http://{http_addr}");

    let state = Arc::new(MockState {
        ws_url,
        base_url: base_url.clone(),
    });
    let app = Router::new()
        .route("/.well-known/jmap", get(mock_session))
        .with_state(state);
    tokio::spawn(async move {
        axum::serve(http_listener, app).await.expect("serve http");
    });

    base_url
}

async fn handle_ws(stream: tokio::net::TcpStream, behavior: WsBehavior) {
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
    // Echo the `jmap` subprotocol the client offers, as a real JMAP server does.
    let with_subprotocol = |_req: &Request, mut response: Response| {
        response.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            "jmap".parse().expect("static header value"),
        );
        Ok(response)
    };
    let ws = tokio_tungstenite::accept_hdr_async(stream, with_subprotocol)
        .await
        .expect("ws handshake");
    let (mut tx, mut rx) = ws.split();

    // Saturate the client's bounded push channel before any request is serviced.
    for _ in 0..behavior.flood_pushes {
        if tx.send(push_frame()).await.is_err() {
            return;
        }
    }

    while let Some(Ok(msg)) = rx.next().await {
        match msg {
            Message::Text(text) => {
                let value: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let ty = value.get("@type").and_then(Value::as_str).unwrap_or("");
                match ty {
                    "Request" => {
                        let _ = tx.send(response_for(&value)).await;
                    }
                    // WebSocketPushEnable / WebSocketPushDisable: acknowledge by
                    // doing nothing (no push subscription needed for the repro).
                    _ => {}
                }
            }
            Message::Ping(payload) => {
                let _ = tx.send(Message::Pong(payload)).await;
            }
            Message::Close(_) => return,
            _ => {}
        }
    }
}

async fn connect_gateway(base_url: &str) -> LiveJmapGateway {
    let client = connect_jmap_client(base_url, Some("dev"), "devpass")
        .await
        .expect("connect mock client");
    let gateway = LiveJmapGateway::from_client(client);
    gateway
        .ws()
        .expect("ws configured")
        .ensure_connected()
        .await
        .expect("ws connected");
    gateway
}

/// A single-method request (Email/set destroy) over WS completes. Baseline.
#[tokio::test]
async fn single_method_ws_request_completes() {
    let base_url = spawn_mock(WsBehavior { flood_pushes: 0 }).await;
    let gateway = connect_gateway(&base_url).await;

    let mut request = gateway.client().build();
    request.set_email().destroy(["E-old"]);

    let response = tokio::time::timeout(Duration::from_secs(3), gateway.send_request(request))
        .await
        .expect("single-method WS request should not hang")
        .expect("single-method WS request should succeed");
    assert_eq!(response.request_id(), Some("0"));
}

/// A multi-method request (Mailbox/get + Identity/get + Email/set) over WS
/// completes -- mirrors the save_draft flush shape.
#[tokio::test]
async fn multi_method_ws_request_completes() {
    let base_url = spawn_mock(WsBehavior { flood_pushes: 0 }).await;
    let gateway = connect_gateway(&base_url).await;

    let mut request = gateway.client().build();
    request.get_mailbox();
    request.get_identity();
    request.set_email().create();

    let response = tokio::time::timeout(Duration::from_secs(3), gateway.send_request(request))
        .await
        .expect("multi-method WS request should not hang")
        .expect("multi-method WS request should succeed");
    assert_eq!(response.request_id(), Some("0"));
}

/// The real M67 root cause: when push notifications saturate the shared
/// connection's bounded push buffer and nothing is draining them, the WS reader
/// task blocks delivering a push and can no longer correlate API responses --
/// so a subsequent request hangs. (The "multi-method" framing was incidental:
/// the save_draft flush simply ran after the buffer had filled.)
#[tokio::test]
async fn ws_request_completes_despite_push_saturation() {
    // Flood well past the fork's 64-slot push channel with no consumer draining.
    let base_url = spawn_mock(WsBehavior { flood_pushes: 200 }).await;
    let gateway = connect_gateway(&base_url).await;

    // Give the reader a moment to buffer the flood and (pre-fix) wedge.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let mut request = gateway.client().build();
    request.set_email().create();

    let response = tokio::time::timeout(Duration::from_secs(3), gateway.send_request(request))
        .await
        .expect("WS request must not hang behind a saturated push buffer")
        .expect("WS request should succeed");
    assert_eq!(response.request_id(), Some("0"));
}

/// The decoupling that fixes the hang must not drop push delivery: pushes the
/// server emits on the shared connection still reach a `next_push` consumer.
#[tokio::test]
async fn push_notifications_still_delivered_on_shared_connection() {
    let base_url = spawn_mock(WsBehavior { flood_pushes: 3 }).await;
    let gateway = connect_gateway(&base_url).await;
    let ws = gateway.ws().expect("ws configured");

    // An interleaved API request still completes...
    let mut request = gateway.client().build();
    request.set_email().create();
    let response = tokio::time::timeout(Duration::from_secs(3), gateway.send_request(request))
        .await
        .expect("request should not hang")
        .expect("request should succeed");
    assert_eq!(response.request_id(), Some("0"));

    // ...and the three pushes are delivered to the push consumer, in order.
    for _ in 0..3 {
        let push = tokio::time::timeout(Duration::from_secs(3), ws.next_push())
            .await
            .expect("push should be delivered, not hang")
            .expect("push stream should not end")
            .expect("push should not be an error");
        match push {
            jmap_client::PushObject::StateChange { .. } => {}
            other => panic!("unexpected push object: {other:?}"),
        }
    }
}
