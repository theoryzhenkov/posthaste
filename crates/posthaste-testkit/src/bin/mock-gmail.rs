//! Runnable mock Gmail IMAP server for local development.
//!
//! Serves the same Gmail-shaped IMAP fixture the testkit uses (X-GM-EXT-1 +
//! CONDSTORE + QRESYNC, RFC 7162 `VANISHED`), so you can add a Gmail (IMAP)
//! account in the dev app pointed at it and exercise the real sync path —
//! discovery, initial snapshot, and CONDSTORE/QRESYNC delta — without a real
//! Gmail account. Wired into the dev stack via `POSTHASTE_DEV_GMAIL=1 just dev`.
//!
//! Ports come from `POSTHASTE_MOCK_GMAIL_IMAP_PORT` (default 11430) and
//! `POSTHASTE_MOCK_GMAIL_CONTROL_PORT` (default 11431). Drive it with:
//!
//! ```text
//! curl -XPOST 'http://127.0.0.1:11431/deliver?subject=Hello'
//! curl -XPOST 'http://127.0.0.1:11431/vanish?subject=Replaced'
//! ```

fn port_from_env(key: &str, default: u16) -> u16 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let imap_port = port_from_env("POSTHASTE_MOCK_GMAIL_IMAP_PORT", 11430);
    let control_port = port_from_env("POSTHASTE_MOCK_GMAIL_CONTROL_PORT", 11431);
    posthaste_testkit::serve_mock_gmail(imap_port, control_port).await
}
