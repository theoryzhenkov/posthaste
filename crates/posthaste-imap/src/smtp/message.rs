use base64::Engine;
use lettre::message::{header, Attachment, Body, Mailbox, MultiPart, SinglePart};
use lettre::{Address, Message};
use posthaste_domain_model::{Recipient, SendMessageRequest};
use pulldown_cmark::{html, Event, Options, Parser, Tag, TagEnd};

use crate::smtp::SmtpConnectionConfig;
use crate::ImapAdapterError;

/// Build the RFC 5322 message sent through SMTP submission.
///
/// The MIME shape mirrors the JMAP send path: Markdown source is sent as the
/// plain text alternative and rendered HTML is sent as the HTML alternative.
///
/// `message_id` is the stable RFC5322 Message-ID value (no angle brackets;
/// lettre adds them) derived from the send's idempotency key — constant across
/// retries so a de-duplicating MTA drops a second copy of the same send (D85).
/// `None` falls back to lettre generating a fresh id (the non-idempotent batch
/// / draft-preview paths).
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
/// @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
pub fn build_smtp_message(
    config: &SmtpConnectionConfig,
    request: &SendMessageRequest,
    message_id: Option<&str>,
) -> Result<Message, ImapAdapterError> {
    let mut builder = Message::builder()
        .from(smtp_sender_mailbox(config, request.from.as_ref())?)
        .subject(request.subject.clone())
        .message_id(message_id.map(str::to_string));

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
    attachment: &posthaste_domain_model::SendMessageAttachment,
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

fn normalized_attachment_mime_type(
    attachment: &posthaste_domain_model::SendMessageAttachment,
) -> &str {
    let mime_type = attachment.mime_type.trim();
    if mime_type.is_empty() {
        "application/octet-stream"
    } else {
        mime_type
    }
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

/// Stable RFC5322 Message-ID *value* (no angle brackets) derived from a send's
/// idempotency key (the outbox op id), so every retry carries the same id and a
/// de-duplicating MTA drops the duplicate (D85). Matches the JMAP send token
/// (`phsend-<key>`) so one operation yields one identity across transports. The
/// domain is the account's own sender domain.
///
/// @spec docs/eph/RFC-L2-provider-reliability#32-send-exactly-once
pub fn smtp_stable_message_id(idempotency_key: &str, config: &SmtpConnectionConfig) -> String {
    let token = posthaste_domain_model::send_identity_token(idempotency_key);
    let domain = config
        .sender_email
        .rsplit_once('@')
        .map(|(_, domain)| domain)
        .filter(|domain| !domain.is_empty())
        .unwrap_or("posthaste.local");
    format!("{token}@{domain}")
}

fn smtp_message_id_header_value(id: &str) -> String {
    let id = id.trim();
    if id.starts_with('<') && id.ends_with('>') {
        id.to_string()
    } else {
        format!("<{id}>")
    }
}
