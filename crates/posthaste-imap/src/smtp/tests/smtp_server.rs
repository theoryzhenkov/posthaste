use posthaste_domain::{ProviderAuthKind, ProviderHint, SendMessageRequest, TransportSecurity};

use super::*;

#[tokio::test]
async fn submits_message_to_smtp_server_and_returns_raw_copy() {
    let (addr, captured) = spawn_fake_smtp_server().await;
    let config = SmtpConnectionConfig {
        host: "127.0.0.1".to_string(),
        port: addr.port(),
        security: TransportSecurity::Plain,
        sender_name: Some("Alice".to_string()),
        sender_email: "alice@example.test".to_string(),
        username: "alice@example.test".to_string(),
        secret: "secret".to_string(),
        auth: ProviderAuthKind::Password,
        provider: ProviderHint::Generic,
    };
    let request = SendMessageRequest {
        from: None,
        to: vec![recipient(Some("Bob"), "bob@example.test")],
        cc: Vec::new(),
        bcc: vec![recipient(Some("Dana"), "dana@example.test")],
        subject: "Captured".to_string(),
        body: "Hello from **SMTP**".to_string(),
        ..Default::default()
    };

    let submitted = submit_smtp_message(&config, &request)
        .await
        .expect("SMTP submission");
    let captured = captured.await.expect("fake SMTP captured message");
    let raw = String::from_utf8(submitted.raw_message).expect("raw message is UTF-8");

    assert!(captured
        .commands
        .iter()
        .any(|command| { command.eq_ignore_ascii_case("RCPT TO:<bob@example.test>") }));
    assert!(captured
        .commands
        .iter()
        .any(|command| { command.eq_ignore_ascii_case("RCPT TO:<dana@example.test>") }));
    assert!(captured.data.contains("Subject: Captured"));
    assert!(captured
        .data
        .contains("Content-Type: multipart/alternative;"));
    assert!(!captured.data.contains("Bcc:"));
    assert!(raw.contains("Subject: Captured"));
    assert!(!raw.contains("Bcc:"));
}
