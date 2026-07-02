//! End-to-end TLS serving: build the acceptor from a self-signed fixture, serve
//! a trivial router through [`posthaste_api::tls::TlsListener`], and prove that
//!   - an HTTPS client that trusts the fixture cert gets a 200, and
//!   - a plaintext HTTP request to the TLS port fails (the port really is TLS).
//!
//! This exercises the actual `axum::serve(TlsListener, ..)` path the daemon uses,
//! without booting the full runtime.
//!
//! @spec docs/eph/DESIGN-L1-trust-model

use std::path::PathBuf;

use axum::routing::get;
use axum::Router;
use posthaste_api::tls::{build_tls_acceptor, TlsListener};
use posthaste_config::TlsConfig;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/tls")
        .join(name)
}

fn fixture_tls_config() -> TlsConfig {
    TlsConfig {
        cert_path: fixture("localhost.crt"),
        key_path: fixture("localhost.key"),
    }
}

/// Bind an ephemeral TLS listener serving `GET /ping -> "pong"`, returning its
/// `127.0.0.1:<port>` address. Panics if the fixture cert is missing.
async fn spawn_tls_server() -> std::net::SocketAddr {
    let cfg = fixture_tls_config();
    assert!(
        cfg.cert_path.exists(),
        "missing TLS fixture {} — generate with openssl (see tests/tls)",
        cfg.cert_path.display()
    );
    let acceptor = build_tls_acceptor(&cfg).expect("build acceptor");
    let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = tcp.local_addr().unwrap();
    let listener = TlsListener::new(tcp, acceptor);
    let app = Router::new().route("/ping", get(|| async { "pong" }));
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

fn client_trusting_fixture() -> reqwest::Client {
    // Trust the fixture CA; the server presents the leaf `localhost.crt` signed
    // by it. (Trusting the leaf directly fails webpki's CaUsedAsEndEntity check.)
    let pem = std::fs::read(fixture("ca.crt")).unwrap();
    let cert = reqwest::Certificate::from_pem(&pem).unwrap();
    reqwest::Client::builder()
        .add_root_certificate(cert)
        .build()
        .unwrap()
}

#[tokio::test]
async fn https_client_trusting_cert_succeeds() {
    let addr = spawn_tls_server().await;
    let client = client_trusting_fixture();
    let resp = client
        .get(format!("https://localhost:{}/ping", addr.port()))
        .send()
        .await
        .expect("https request should succeed");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "pong");
}

#[tokio::test]
async fn plaintext_http_to_tls_port_fails() {
    let addr = spawn_tls_server().await;
    // A plaintext HTTP request to a TLS port must not yield a usable response.
    let result = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{}/ping", addr.port()))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;
    assert!(
        result.is_err(),
        "plaintext HTTP to the TLS port unexpectedly succeeded"
    );
}

#[tokio::test]
async fn a_stalled_handshake_does_not_block_other_clients() {
    // Regression for the accept-loop DoS: open a raw TCP connection that never
    // sends a TLS ClientHello, then make a real HTTPS request. If handshakes ran
    // inline on the accept loop, the stalled socket would wedge the listener and
    // the real request would hang; with off-loop handshakes it succeeds.
    let addr = spawn_tls_server().await;
    let _stalled = tokio::net::TcpStream::connect(addr)
        .await
        .expect("raw connect");
    // Hold the stalled socket open for the duration of the real request.
    let client = client_trusting_fixture();
    let resp = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client
            .get(format!("https://localhost:{}/ping", addr.port()))
            .send(),
    )
    .await
    .expect("request must not hang behind the stalled handshake")
    .expect("https request should succeed");
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn untrusting_client_rejects_self_signed_cert() {
    let addr = spawn_tls_server().await;
    // Default client (no fixture root) must reject the self-signed cert.
    let result = reqwest::Client::new()
        .get(format!("https://localhost:{}/ping", addr.port()))
        .send()
        .await;
    assert!(
        result.is_err(),
        "client without the fixture root should reject the self-signed cert"
    );
}
