use mail_parser::{Address, MessageParser};
use posthaste_domain_model::{
    format_forwarded_body, recipients_to_header, ImapMessageLocation, Recipient, ReplyContext,
};

use imap_client::client::tokio::Client as ImapClient;

use crate::body::fetch_raw_message_by_location;
use crate::ImapAdapterError;

/// Fetch and parse IMAP reply/forward metadata from the authoritative message.
///
/// @spec docs/L1-compose#reply-quoting
pub(crate) async fn fetch_imap_reply_context_by_location(
    client: &mut ImapClient,
    mailbox_name: &str,
    location: &ImapMessageLocation,
) -> Result<ReplyContext, ImapAdapterError> {
    let raw_mime = fetch_raw_message_by_location(client, mailbox_name, location).await?;
    imap_reply_context_from_raw_mime(raw_mime)
}

pub fn imap_reply_context_from_raw_mime(
    raw_mime: Vec<u8>,
) -> Result<ReplyContext, ImapAdapterError> {
    let parsed = MessageParser::default()
        .parse(&raw_mime)
        .ok_or(ImapAdapterError::ParseMessageBody)?;
    let subject = parsed.subject().unwrap_or("(no subject)");
    let plain_body = parsed.body_text(0);
    let quoted_body = plain_body.as_ref().map(|body| {
        body.lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    });
    let original_from = parsed
        .from()
        .map(addresses_to_recipients)
        .unwrap_or_default();
    let original_to = parsed.to().map(addresses_to_recipients).unwrap_or_default();
    let original_date = parsed.date().map(|date| date.to_rfc3339());
    let forwarded_body = Some(format_forwarded_body(
        recipients_to_header(&original_from).as_deref(),
        original_date.as_deref(),
        Some(subject),
        recipients_to_header(&original_to).as_deref(),
        plain_body.as_deref().unwrap_or_default(),
    ));

    Ok(ReplyContext {
        to: original_from.clone(),
        cc: parsed.cc().map(addresses_to_recipients).unwrap_or_default(),
        original_to,
        reply_subject: prefix_subject("Re:", subject),
        forward_subject: prefix_subject("Fwd:", subject),
        quoted_body,
        forwarded_body,
        in_reply_to: parsed.message_id().map(str::to_string),
        references: parsed.references().as_text_list().map(|items| {
            items
                .iter()
                .map(|item| item.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        }),
        original_from,
        original_date,
    })
}

fn addresses_to_recipients(addresses: &Address<'_>) -> Vec<Recipient> {
    addresses
        .iter()
        .filter_map(|address| {
            Some(Recipient {
                name: address.name.as_ref().map(|name| name.to_string()),
                email: address.address.as_ref()?.to_string(),
            })
        })
        .collect()
}

fn prefix_subject(prefix: &str, subject: &str) -> String {
    if subject
        .to_ascii_lowercase()
        .starts_with(&prefix.to_ascii_lowercase())
    {
        subject.to_string()
    } else {
        format!("{prefix} {subject}")
    }
}

#[cfg(test)]
mod tests;
