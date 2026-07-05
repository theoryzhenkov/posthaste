use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use posthaste_domain_model::AccountId;
use posthaste_domain_model::{
    AccountDriver, AccountSettings, AccountTransportSettings, ProviderAuthKind, ProviderHint,
    Recipient, SecretKind, SecretRef, SendMessageRequest, SmtpTransportSettings, TransportSecurity,
    RFC3339_EPOCH,
};

use super::*;

fn test_config() -> SmtpConnectionConfig {
    SmtpConnectionConfig {
        host: "smtp.example.test".to_string(),
        port: 587,
        security: TransportSecurity::StartTls,
        sender_name: None,
        sender_email: "alice@example.test".to_string(),
        username: "alice@example.test".to_string(),
        secret: "secret".to_string(),
        auth: ProviderAuthKind::Password,
        provider: ProviderHint::Generic,
    }
}

fn recipient(name: Option<&str>, email: &str) -> Recipient {
    Recipient {
        name: name.map(str::to_string),
        email: email.to_string(),
    }
}

fn test_account(
    full_name: Option<&str>,
    email_patterns: Vec<&str>,
    username: &str,
) -> AccountSettings {
    AccountSettings {
        id: AccountId::from("primary"),
        name: "Primary".to_string(),
        full_name: full_name.map(str::to_string),
        signature: None,
        email_patterns: email_patterns.into_iter().map(str::to_string).collect(),
        driver: AccountDriver::ImapSmtp,
        enabled: true,
        appearance: None,
        transport: AccountTransportSettings {
            provider: ProviderHint::Generic,
            auth: ProviderAuthKind::Password,
            username: Some(username.to_string()),
            secret_ref: Some(SecretRef {
                kind: SecretKind::Env,
                key: "POSTHASTE_TEST_SECRET".to_string(),
            }),
            smtp: Some(SmtpTransportSettings {
                host: "smtp.example.test".to_string(),
                port: 587,
                security: TransportSecurity::StartTls,
            }),
            ..Default::default()
        },
        created_at: RFC3339_EPOCH.to_string(),
        updated_at: RFC3339_EPOCH.to_string(),
    }
}

#[derive(Debug)]
struct CapturedSmtpMessage {
    commands: Vec<String>,
    data: String,
}

async fn spawn_fake_smtp_server() -> (std::net::SocketAddr, oneshot::Receiver<CapturedSmtpMessage>)
{
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake SMTP server");
    let addr = listener.local_addr().expect("fake SMTP local addr");
    let (tx, rx) = oneshot::channel();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept SMTP client");
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        let mut commands = Vec::new();
        let mut data = String::new();

        writer.write_all(b"220 localhost ESMTP\r\n").await.unwrap();
        loop {
            line.clear();
            if reader.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            let command = line.trim_end_matches(['\r', '\n']).to_string();
            commands.push(command.clone());
            let upper = command.to_ascii_uppercase();

            if upper.starts_with("EHLO") || upper.starts_with("HELO") {
                writer
                    .write_all(b"250-localhost\r\n250-AUTH PLAIN LOGIN\r\n250 OK\r\n")
                    .await
                    .unwrap();
            } else if upper.starts_with("AUTH") {
                writer.write_all(b"235 2.7.0 ok\r\n").await.unwrap();
            } else if upper.starts_with("MAIL FROM") || upper.starts_with("RCPT TO") {
                writer.write_all(b"250 2.1.0 ok\r\n").await.unwrap();
            } else if upper == "DATA" {
                writer
                    .write_all(b"354 end with <CRLF>.<CRLF>\r\n")
                    .await
                    .unwrap();
                loop {
                    line.clear();
                    reader.read_line(&mut line).await.unwrap();
                    let data_line = line.trim_end_matches(['\r', '\n']);
                    if data_line == "." {
                        break;
                    }
                    data.push_str(data_line);
                    data.push('\n');
                }
                writer.write_all(b"250 2.0.0 queued\r\n").await.unwrap();
            } else if upper == "QUIT" {
                writer.write_all(b"221 2.0.0 bye\r\n").await.unwrap();
                break;
            } else {
                writer.write_all(b"250 ok\r\n").await.unwrap();
            }
        }

        let _ = tx.send(CapturedSmtpMessage { commands, data });
    });

    (addr, rx)
}

/// A fake SMTP server that speaks the transaction up to the DATA terminator,
/// then DROPS the connection instead of sending the final `250` — modelling the
/// duplicate-send danger case: the MTA may have accepted the message, but the
/// completion code is never read, so the send's fate is unknown.
async fn spawn_smtp_server_dropping_after_data() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake SMTP server");
    let addr = listener.local_addr().expect("fake SMTP local addr");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept SMTP client");
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        writer.write_all(b"220 localhost ESMTP\r\n").await.unwrap();
        loop {
            line.clear();
            if reader.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            let upper = line.trim_end_matches(['\r', '\n']).to_ascii_uppercase();
            if upper.starts_with("EHLO") || upper.starts_with("HELO") {
                writer
                    .write_all(b"250-localhost\r\n250-AUTH PLAIN LOGIN\r\n250 OK\r\n")
                    .await
                    .unwrap();
            } else if upper.starts_with("AUTH") {
                writer.write_all(b"235 2.7.0 ok\r\n").await.unwrap();
            } else if upper.starts_with("MAIL FROM") || upper.starts_with("RCPT TO") {
                writer.write_all(b"250 2.1.0 ok\r\n").await.unwrap();
            } else if upper == "DATA" {
                writer
                    .write_all(b"354 end with <CRLF>.<CRLF>\r\n")
                    .await
                    .unwrap();
                // Read the message body and, on the terminating `.`, drop the
                // connection WITHOUT the closing 250 — the accept is unknown.
                loop {
                    line.clear();
                    if reader.read_line(&mut line).await.unwrap() == 0 {
                        return;
                    }
                    if line.trim_end_matches(['\r', '\n']) == "." {
                        return; // drops `writer`/`reader` → socket closed
                    }
                }
            } else {
                writer.write_all(b"250 ok\r\n").await.unwrap();
            }
        }
    });

    addr
}

fn plain_config(port: u16) -> SmtpConnectionConfig {
    SmtpConnectionConfig {
        host: "127.0.0.1".to_string(),
        port,
        security: TransportSecurity::Plain,
        sender_name: Some("Alice".to_string()),
        sender_email: "alice@example.test".to_string(),
        username: "alice@example.test".to_string(),
        secret: "secret".to_string(),
        auth: ProviderAuthKind::Password,
        provider: ProviderHint::Generic,
    }
}

fn probe_request() -> SendMessageRequest {
    SendMessageRequest {
        from: None,
        to: vec![recipient(Some("Bob"), "bob@example.test")],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: "Probe".to_string(),
        body: "hi".to_string(),
        ..Default::default()
    }
}

/// A drop AFTER the DATA final `.` (the MTA may have accepted): the send's fate
/// is unknown, so the gateway must classify it dispatch-uncertain — the outbox
/// parks it, never blind-resending it into a duplicate (DP-C5/C6, O5/D86).
#[tokio::test]
async fn smtp_drop_after_data_is_dispatch_uncertain() {
    let addr = spawn_smtp_server_dropping_after_data().await;
    let error = submit_smtp_message(&plain_config(addr.port()), &probe_request(), None)
        .await
        .expect_err("a dropped post-DATA connection must fail the send");
    // The imap-layer classification; `imap_error_to_gateway` then routes this
    // variant to `GatewayError::DispatchUncertain` (its exhaustive match).
    assert!(
        matches!(error, crate::ImapAdapterError::SmtpDispatchUncertain(_)),
        "a post-DATA transport drop must be SmtpDispatchUncertain (park, never resend), got {error:?}"
    );
}

/// A pre-connect failure (nothing listening): the message bytes never left the
/// socket, so it is a safe retryable transient — a genuinely offline send still
/// auto-retries rather than parking.
#[tokio::test]
async fn smtp_pre_connect_failure_is_retryable_network() {
    // Bind then immediately drop the listener to obtain a definitely-closed port.
    let closed_port = {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        listener.local_addr().expect("addr").port()
    };
    let error = submit_smtp_message(&plain_config(closed_port), &probe_request(), None)
        .await
        .expect_err("connecting to a closed port must fail");
    // A pre-write connection failure stays `Smtp` → routes to a retryable
    // `GatewayError::Network`, so a genuinely offline send auto-retries.
    assert!(
        matches!(error, crate::ImapAdapterError::Smtp(_)),
        "a pre-connect failure must be a retryable Smtp error, got {error:?}"
    );
}

mod message_rendering;
mod smtp_server;
