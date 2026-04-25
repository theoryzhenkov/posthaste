use lettre::message::{header, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{Address, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use posthaste_domain::{
    AccountTransportSettings, ProviderAuthKind, Recipient, SendMessageRequest, TransportSecurity,
};
use pulldown_cmark::{html, Options, Parser};

use crate::ImapAdapterError;

/// Concrete connection details for one SMTP submission endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmtpConnectionConfig {
    pub host: String,
    pub port: u16,
    pub security: TransportSecurity,
    pub username: String,
    pub secret: String,
    pub auth: ProviderAuthKind,
}

impl SmtpConnectionConfig {
    pub fn from_account_transport(
        transport: &AccountTransportSettings,
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

        Ok(Self {
            host: smtp.host.clone(),
            port: smtp.port,
            security: smtp.security.clone(),
            username: username.to_string(),
            secret,
            auth: transport.auth.clone(),
        })
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
        .subject(request.subject.clone());

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
    let message = build_smtp_message(config, request)?;
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
    let transport = builder
        .port(config.port)
        .credentials(credentials)
        .authentication(mechanisms)
        .build();

    transport.send(message).await?;
    Ok(())
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
    let name = config
        .username
        .split('@')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    smtp_mailbox(name, &config.username)
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
    use posthaste_domain::{
        AccountTransportSettings, ProviderAuthKind, Recipient, SmtpTransportSettings,
        TransportSecurity,
    };

    use super::*;

    #[test]
    fn config_from_transport_requires_smtp_settings() {
        let transport = AccountTransportSettings {
            username: Some("alice@example.test".to_string()),
            ..Default::default()
        };

        let error = SmtpConnectionConfig::from_account_transport(&transport, "secret".to_string())
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
        let transport = AccountTransportSettings {
            username: Some("alice@example.test".to_string()),
            auth: ProviderAuthKind::OAuth2,
            smtp: Some(SmtpTransportSettings {
                host: "smtp.example.test".to_string(),
                port: 587,
                security: TransportSecurity::StartTls,
            }),
            ..Default::default()
        };

        let config =
            SmtpConnectionConfig::from_account_transport(&transport, "access-token".to_string())
                .expect("SMTP config");

        assert_eq!(config.auth, ProviderAuthKind::OAuth2);
        assert_eq!(config.secret, "access-token");
    }

    fn test_config() -> SmtpConnectionConfig {
        SmtpConnectionConfig {
            host: "smtp.example.test".to_string(),
            port: 587,
            security: TransportSecurity::StartTls,
            username: "alice@example.test".to_string(),
            secret: "secret".to_string(),
            auth: ProviderAuthKind::Password,
        }
    }

    fn recipient(name: Option<&str>, email: &str) -> Recipient {
        Recipient {
            name: name.map(str::to_string),
            email: email.to_string(),
        }
    }
}
