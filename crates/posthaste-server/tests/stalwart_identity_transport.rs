//! Focused reproduction: `Identity/get` must return the account's identities
//! over BOTH transports. The live app routes interactive calls (identity, send,
//! draft) over the shared WebSocket once push connects, while the provider
//! parity test only ever uses HTTP — so a WS-only identity regression slips
//! through. This isolates the two transports against real Stalwart.
//!
//! Gated behind `POSTHASTE_STALWART_INTEGRATION=1` like the parity suite.

// The fixture/util modules are shared with the parity suite; this test only uses
// part of their surface, so silence dead-code warnings for the unused helpers.
#[allow(dead_code)]
#[path = "stalwart_provider_parity/fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "stalwart_provider_parity/util.rs"]
mod util;

use posthaste_domain::{AccountId, MailGateway};
use posthaste_engine::LiveJmapGateway;

use crate::fixture::StalwartFixture;

#[tokio::test]
async fn identity_get_resolves_over_http_and_websocket() {
    if std::env::var("POSTHASTE_STALWART_INTEGRATION").as_deref() != Ok("1") {
        eprintln!("skipping Stalwart integration; set POSTHASTE_STALWART_INTEGRATION=1");
        return;
    }

    let stalwart = StalwartFixture::start();
    let account = AccountId::from("jmap-stalwart");
    let gateway = LiveJmapGateway::connect(&stalwart.http_url, Some("dev"), &stalwart.password)
        .await
        .expect("JMAP gateway should connect");

    // 1. HTTP transport (no WS connected yet): identity resolves.
    let http_identity = gateway
        .fetch_identity(&account)
        .await
        .expect("Identity/get over HTTP should resolve an identity");
    eprintln!(
        "HTTP identity: id={:?} name={:?} email={:?}",
        http_identity.id, http_identity.name, http_identity.email
    );
    assert!(
        !http_identity.email.is_empty(),
        "HTTP identity should carry an email"
    );

    // 2. Connect the shared WebSocket (as the live app does when push starts),
    //    so subsequent interactive calls route over WS.
    let transports = gateway.push_transports();
    let mut connected_ws = false;
    for transport in transports {
        if transport.name() == "ws" {
            transport
                .open(&account, None)
                .await
                .expect("WS push open should connect the shared socket");
            connected_ws = true;
        }
    }
    eprintln!("server advertised WS push: {connected_ws}");

    // 3. WS transport: identity must resolve identically. This is the path the
    //    live app uses for send/draft, where the regression appears.
    let ws_identity = gateway
        .fetch_identity(&account)
        .await
        .expect("Identity/get over WebSocket should resolve an identity");
    eprintln!(
        "WS identity:   id={:?} name={:?} email={:?}",
        ws_identity.id, ws_identity.name, ws_identity.email
    );
    assert_eq!(
        ws_identity.email, http_identity.email,
        "WS identity must match the HTTP identity"
    );
}
