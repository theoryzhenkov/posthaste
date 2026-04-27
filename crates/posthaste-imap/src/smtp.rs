use std::num::NonZeroU32;

use imap_client::imap_types::flag::Flag;
use lettre::message::{header, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{Address, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use posthaste_domain::{
    AccountSettings, AccountTransportSettings, ProviderAuthKind, ProviderHint, Recipient,
    SendMessageRequest, TransportSecurity,
};
use pulldown_cmark::{html, Options, Parser};

use crate::discovery::connect_authenticated_client;
use crate::ImapAdapterError;
use crate::ImapConnectionConfig;

/// Concrete connection details for one SMTP submission endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmtpConnectionConfig {
    pub host: String,
    pub port: u16,
    pub security: TransportSecurity,
    pub sender_name: Option<String>,
    pub sender_email: String,
    pub username: String,
    pub secret: String,
    pub auth: ProviderAuthKind,
    pub provider: ProviderHint,
}

impl SmtpConnectionConfig {
    pub fn from_account_settings(
        account: &AccountSettings,
        secret: String,
    ) -> Result<Self, ImapAdapterError> {
        Self::from_parts(
            &account.transport,
            account.full_name.as_deref(),
            concrete_sender_email(&account.email_patterns),
            secret,
        )
    }

    fn from_parts(
        transport: &AccountTransportSettings,
        sender_name: Option<&str>,
        sender_email: Option<String>,
        secret: String,
    ) -> Result<Self, ImapAdapterError> {
        let smtp = transport
            .smtp
            .as_ref()
            .ok_or(ImapAdapterError::MissingSmtpTransport)?;
        let username = transport
            .username
            .as_deref()
            .map(str::trim)
            .filter(|username| !username.is_empty())
            .ok_or(ImapAdapterError::MissingUsername)?;
        if secret.trim().is_empty() {
            return Err(ImapAdapterError::MissingSecret);
        }
        let sender_email = sender_email.ok_or(ImapAdapterError::MissingSmtpSenderEmail)?;
        let sender_name = sender_name.and_then(|name| {
            let name = name.trim();
            (!name.is_empty()).then(|| name.to_string())
        });

        Ok(Self {
            host: smtp.host.clone(),
            port: smtp.port,
            security: smtp.security.clone(),
            sender_name,
            sender_email,
            username: username.to_string(),
            secret,
            auth: transport.auth.clone(),
            provider: transport.provider.clone(),
        })
    }
}

fn concrete_sender_email<'a>(emails: impl IntoIterator<Item = &'a String>) -> Option<String> {
    emails.into_iter().find_map(|email| {
        let email = email.trim();
        if email.is_empty() || email.contains('*') {
            return None;
        }
        email.parse::<Address>().is_ok().then(|| email.to_string())
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SmtpSentCopyStrategy {
    ProviderManaged,
    AppendToSentMailbox,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmittedSmtpMessage {
    pub raw_message: Vec<u8>,
}

pub fn smtp_sent_copy_strategy(provider: &ProviderHint) -> SmtpSentCopyStrategy {
    match provider {
        ProviderHint::Gmail | ProviderHint::Outlook => SmtpSentCopyStrategy::ProviderManaged,
        ProviderHint::Generic | ProviderHint::Icloud => SmtpSentCopyStrategy::AppendToSentMailbox,
    }
}

/// Build the RFC 5322 message sent through SMTP submission.
///
/// The MIME shape mirrors the JMAP send path: Markdown source is sent as the
/// plain text alternative and rendered HTML is sent as the HTML alternative.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
pub fn build_smtp_message(
    config: &SmtpConnectionConfig,
    request: &SendMessageRequest,
) -> Result<Message, ImapAdapterError> {
    let mut builder = Message::builder()
        .from(smtp_sender_mailbox(config)?)
        .subject(request.subject.clone())
        .message_id(None);

    for recipient in &request.to {
        builder = builder.to(smtp_mailbox_for_recipient(recipient)?);
    }
    for recipient in &request.cc {
        builder = builder.cc(smtp_mailbox_for_recipient(recipient)?);
    }
    for recipient in &request.bcc {
        builder = builder.bcc(smtp_mailbox_for_recipient(recipient)?);
    }
    if let Some(in_reply_to) = &request.in_reply_to {
        builder = builder.in_reply_to(smtp_message_id_header_value(in_reply_to));
    }
    if let Some(references) = &request.references {
        let references = references
            .split_whitespace()
            .map(smtp_message_id_header_value)
            .collect::<Vec<_>>()
            .join(" ");
        if !references.is_empty() {
            builder = builder.references(references);
        }
    }

    let html_body = render_smtp_markdown(&request.body);
    Ok(builder.multipart(
        MultiPart::alternative()
            .singlepart(
                SinglePart::builder()
                    .header(header::ContentType::TEXT_PLAIN)
                    .body(request.body.clone()),
            )
            .singlepart(
                SinglePart::builder()
                    .header(header::ContentType::TEXT_HTML)
                    .body(html_body),
            ),
    )?)
}

/// Send one message through the configured SMTP endpoint.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
pub async fn send_smtp_message(
    config: &SmtpConnectionConfig,
    request: &SendMessageRequest,
) -> Result<(), ImapAdapterError> {
    submit_smtp_message(config, request).await?;
    Ok(())
}

/// Submit one message and return the exact RFC 5322 bytes accepted by SMTP.
pub async fn submit_smtp_message(
    config: &SmtpConnectionConfig,
    request: &SendMessageRequest,
) -> Result<SubmittedSmtpMessage, ImapAdapterError> {
    let message = build_smtp_message(config, request)?;
    let raw_message = message.formatted();
    smtp_transport(config)?.send(message).await?;

    Ok(SubmittedSmtpMessage { raw_message })
}

/// Append the accepted outbound message to an IMAP Sent mailbox.
///
/// This is only used when provider policy says SMTP submission does not create
/// a server-side Sent copy. The message is appended with `\Seen`.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
pub async fn append_smtp_sent_copy(
    config: &ImapConnectionConfig,
    sent_mailbox_name: &str,
    raw_message: &[u8],
) -> Result<Option<NonZeroU32>, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    client.refresh_capabilities().await?;
    client
        .appenduid_or_fallback(sent_mailbox_name, [Flag::Seen], raw_message)
        .await
        .map_err(ImapAdapterError::from)
}

fn smtp_transport(
    config: &SmtpConnectionConfig,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, ImapAdapterError> {
    let credentials = Credentials::new(config.username.clone(), config.secret.clone());
    let builder = match config.security {
        TransportSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&config.host)?,
        TransportSecurity::StartTls => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.host)?
        }
        TransportSecurity::Plain => {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(config.host.clone())
        }
    };
    let mechanisms = match config.auth {
        ProviderAuthKind::Password | ProviderAuthKind::AppPassword => {
            vec![Mechanism::Plain, Mechanism::Login]
        }
        ProviderAuthKind::OAuth2 => vec![Mechanism::Xoauth2],
    };
    Ok(builder
        .port(config.port)
        .credentials(credentials)
        .authentication(mechanisms)
        .build())
}

/// Render Markdown to the same minimal HTML document shape used by JMAP sends.
///
/// @spec docs/L1-compose#supported-markdown-subset
/// @spec docs/L1-compose#html-output-rules
pub fn render_smtp_markdown(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>{html_output}</body></html>"
    )
}

/// Convert a domain recipient to a typed SMTP mailbox.
pub fn smtp_mailbox_for_recipient(recipient: &Recipient) -> Result<Mailbox, ImapAdapterError> {
    smtp_mailbox(recipient.name.clone(), &recipient.email)
}

fn smtp_sender_mailbox(config: &SmtpConnectionConfig) -> Result<Mailbox, ImapAdapterError> {
    let name = config.sender_name.clone().or_else(|| {
        config
            .sender_email
            .split('@')
            .next()
            .filter(|name| !name.is_empty())
            .map(str::to_string)
    });
    smtp_mailbox(name, &config.sender_email)
}

fn smtp_mailbox(name: Option<String>, email: &str) -> Result<Mailbox, ImapAdapterError> {
    let address =
        email
            .parse::<Address>()
            .map_err(|error| ImapAdapterError::InvalidSmtpAddress {
                address: email.to_string(),
                reason: error.to_string(),
            })?;
    let name = name.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    });

    Ok(Mailbox::new(name, address))
}

fn smtp_message_id_header_value(id: &str) -> String {
    let id = id.trim();
    if id.starts_with('<') && id.ends_with('>') {
        id.to_string()
    } else {
        format!("<{id}>")
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    use posthaste_domain::{
        AccountDriver, AccountId, AccountSettings, AccountTransportSettings, ProviderAuthKind,
        ProviderHint, Recipient, SecretKind, SecretRef, SmtpTransportSettings, TransportSecurity,
        RFC3339_EPOCH,
    };

    use super::*;

    #[test]
    fn config_from_account_settings_requires_smtp_settings() {
        let mut account = test_account(None, vec!["alice@example.test"], "alice-login");
        account.transport.smtp = None;

        let error = SmtpConnectionConfig::from_account_settings(&account, "secret".to_string())
            .expect_err("SMTP settings should be required");

        assert!(matches!(error, ImapAdapterError::MissingSmtpTransport));
    }

    #[test]
    fn builds_multipart_message_with_threading_headers_and_hidden_bcc() {
        let config = test_config();
        let request = SendMessageRequest {
            to: vec![recipient(Some("Bob"), "bob@example.test")],
            cc: vec![recipient(None, "carol@example.test")],
            bcc: vec![recipient(Some("Dana"), "dana@example.test")],
            subject: "Status".to_string(),
            body: "Hello **world**".to_string(),
            in_reply_to: Some("original@example.test".to_string()),
            references: Some("root@example.test original@example.test".to_string()),
        };

        let message = build_smtp_message(&config, &request).expect("SMTP message");
        let formatted = String::from_utf8(message.formatted()).expect("message is UTF-8");

        assert!(formatted.contains("From: alice <alice@example.test>"));
        assert!(formatted.contains("To: Bob <bob@example.test>"));
        assert!(formatted.contains("Cc: carol@example.test"));
        assert!(formatted.contains("Subject: Status"));
        assert!(formatted.contains("Message-ID: <"));
        assert!(formatted.contains("Date: "));
        assert!(formatted.contains("In-Reply-To: <original@example.test>"));
        assert!(formatted.contains("References: <root@example.test> <original@example.test>"));
        assert!(formatted.contains("Content-Type: multipart/alternative;"));
        assert!(formatted.contains("Content-Type: text/plain"));
        assert!(formatted.contains("Content-Type: text/html"));
        assert!(formatted.contains("Hello **world**"));
        assert!(render_smtp_markdown(&request.body).contains("<strong>world</strong>"));
        assert!(!formatted.contains("Bcc:"));
        assert!(!formatted.contains("dana@example.test"));
    }

    #[test]
    fn rejects_invalid_recipient_address() {
        let error = smtp_mailbox_for_recipient(&recipient(None, "not an address"))
            .expect_err("invalid address should be rejected");

        assert!(matches!(
            error,
            ImapAdapterError::InvalidSmtpAddress { address, .. } if address == "not an address"
        ));
    }

    #[test]
    fn config_preserves_oauth2_auth_kind_for_xoauth2_sends() {
        let mut account = test_account(None, vec!["alice@example.test"], "alice-login");
        account.transport.provider = ProviderHint::Outlook;
        account.transport.auth = ProviderAuthKind::OAuth2;

        let config =
            SmtpConnectionConfig::from_account_settings(&account, "access-token".to_string())
                .expect("SMTP config");

        assert_eq!(config.auth, ProviderAuthKind::OAuth2);
        assert_eq!(config.provider, ProviderHint::Outlook);
        assert_eq!(config.secret, "access-token");
    }

    #[test]
    fn config_from_account_settings_separates_auth_username_from_sender_email() {
        let account = test_account(
            Some("Alice Example"),
            vec!["*@example.test", "alice@example.test"],
            "alice-login",
        );

        let config = SmtpConnectionConfig::from_account_settings(&account, "secret".to_string())
            .expect("SMTP config");

        assert_eq!(config.username, "alice-login");
        assert_eq!(config.sender_email, "alice@example.test");
        assert_eq!(config.sender_name.as_deref(), Some("Alice Example"));
    }

    #[test]
    fn config_from_account_settings_rejects_missing_sender_email() {
        let account = test_account(None, vec!["*@example.test"], "alice-login");

        let error = SmtpConnectionConfig::from_account_settings(&account, "secret".to_string())
            .expect_err("concrete sender email should be required");

        assert!(matches!(error, ImapAdapterError::MissingSmtpSenderEmail));
    }

    #[test]
    fn config_from_account_settings_does_not_use_email_username_as_sender() {
        let account = test_account(None, Vec::new(), "alice@example.test");

        let error = SmtpConnectionConfig::from_account_settings(&account, "secret".to_string())
            .expect_err("configured sender email should be required");

        assert!(matches!(error, ImapAdapterError::MissingSmtpSenderEmail));
    }

    #[test]
    fn provider_sent_copy_policy_avoids_known_auto_saved_providers() {
        assert_eq!(
            smtp_sent_copy_strategy(&ProviderHint::Gmail),
            SmtpSentCopyStrategy::ProviderManaged
        );
        assert_eq!(
            smtp_sent_copy_strategy(&ProviderHint::Outlook),
            SmtpSentCopyStrategy::ProviderManaged
        );
        assert_eq!(
            smtp_sent_copy_strategy(&ProviderHint::Generic),
            SmtpSentCopyStrategy::AppendToSentMailbox
        );
        assert_eq!(
            smtp_sent_copy_strategy(&ProviderHint::Icloud),
            SmtpSentCopyStrategy::AppendToSentMailbox
        );
    }

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
            to: vec![recipient(Some("Bob"), "bob@example.test")],
            cc: Vec::new(),
            bcc: vec![recipient(Some("Dana"), "dana@example.test")],
            subject: "Captured".to_string(),
            body: "Hello from **SMTP**".to_string(),
            in_reply_to: None,
            references: None,
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

    async fn spawn_fake_smtp_server(
    ) -> (std::net::SocketAddr, oneshot::Receiver<CapturedSmtpMessage>) {
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
}
