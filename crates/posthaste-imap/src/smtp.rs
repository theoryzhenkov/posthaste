use std::num::NonZeroU32;

use base64::Engine;
use imap_client::imap_types::flag::Flag;
use lettre::message::{header, Attachment, Body, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{Address, AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use posthaste_domain::{
    AccountSettings, AccountTransportSettings, ProviderAuthKind, ProviderHint, ProviderProfile,
    Recipient, SendMessageRequest, SmtpSentCopyPolicy, TransportSecurity,
};
use pulldown_cmark::{html, Event, Options, Parser, Tag, TagEnd};

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
    match ProviderProfile::from_hint(provider).smtp().sent_copy() {
        SmtpSentCopyPolicy::ProviderManaged => SmtpSentCopyStrategy::ProviderManaged,
        SmtpSentCopyPolicy::AppendToSentMailbox => SmtpSentCopyStrategy::AppendToSentMailbox,
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
        .from(smtp_sender_mailbox(config, request.from.as_ref())?)
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
    let alternatives = MultiPart::alternative()
        .singlepart(
            SinglePart::builder()
                .header(header::ContentType::TEXT_PLAIN)
                .body(request.body.clone()),
        )
        .singlepart(
            SinglePart::builder()
                .header(header::ContentType::TEXT_HTML)
                .body(html_body),
        );
    if request.attachments.is_empty() {
        return Ok(builder.multipart(alternatives)?);
    }

    let mut mixed = MultiPart::mixed().multipart(alternatives);
    for attachment in &request.attachments {
        mixed = mixed.singlepart(smtp_attachment_part(attachment)?);
    }
    Ok(builder.multipart(mixed)?)
}

fn smtp_attachment_part(
    attachment: &posthaste_domain::SendMessageAttachment,
) -> Result<SinglePart, ImapAdapterError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(attachment.content_base64.trim())
        .map_err(|_| {
            ImapAdapterError::BuildSmtpMessage(format!(
                "attachment {} is not valid base64",
                attachment.filename
            ))
        })?;
    let content_type = header::ContentType::parse(normalized_attachment_mime_type(attachment))
        .map_err(|error| ImapAdapterError::BuildSmtpMessage(error.to_string()))?;
    Ok(
        Attachment::new(attachment.filename.trim().to_string())
            .body(Body::new(bytes), content_type),
    )
}

fn normalized_attachment_mime_type(attachment: &posthaste_domain::SendMessageAttachment) -> &str {
    let mime_type = attachment.mime_type.trim();
    if mime_type.is_empty() {
        "application/octet-stream"
    } else {
        mime_type
    }
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
    let parser = Parser::new_ext(markdown, options).filter_map(|event| match event {
        Event::Html(html) | Event::InlineHtml(html) => Some(Event::Text(html)),
        Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image) => None,
        event => Some(event),
    });
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

fn smtp_sender_mailbox(
    config: &SmtpConnectionConfig,
    from: Option<&Recipient>,
) -> Result<Mailbox, ImapAdapterError> {
    if let Some(from) = from {
        return smtp_mailbox_for_recipient(from);
    }
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
mod tests;
