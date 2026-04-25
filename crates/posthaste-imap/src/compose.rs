use mail_parser::{Address, MessageParser};
use posthaste_domain::{ImapMessageLocation, Recipient, ReplyContext};

use crate::{fetch_raw_message_by_location, ImapAdapterError, ImapConnectionConfig};

/// Fetch and parse IMAP reply/forward metadata from the authoritative message.
///
/// @spec docs/L1-compose#reply-quoting
pub async fn fetch_imap_reply_context_by_location(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    location: &ImapMessageLocation,
) -> Result<ReplyContext, ImapAdapterError> {
    let raw_mime = fetch_raw_message_by_location(config, mailbox_name, location).await?;
    imap_reply_context_from_raw_mime(raw_mime)
}

pub fn imap_reply_context_from_raw_mime(
    raw_mime: Vec<u8>,
) -> Result<ReplyContext, ImapAdapterError> {
    let parsed = MessageParser::default()
        .parse(&raw_mime)
        .ok_or(ImapAdapterError::ParseMessageBody)?;
    let subject = parsed.subject().unwrap_or("(no subject)");
    let quoted_body = parsed.body_text(0).map(|body| {
        body.lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    });

    Ok(ReplyContext {
        to: parsed
            .from()
            .map(addresses_to_recipients)
            .unwrap_or_default(),
        cc: parsed.cc().map(addresses_to_recipients).unwrap_or_default(),
        reply_subject: prefix_subject("Re:", subject),
        forward_subject: prefix_subject("Fwd:", subject),
        quoted_body,
        in_reply_to: parsed.message_id().map(str::to_string),
        references: parsed.references().as_text_list().map(|items| {
            items
                .iter()
                .map(|item| item.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        }),
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
mod tests {
    use super::*;

    #[test]
    fn builds_reply_context_from_raw_mime() {
        let raw = concat!(
            "From: Alice <alice@example.test>\r\n",
            "Cc: Bob <bob@example.test>, carol@example.test\r\n",
            "Subject: Hello\r\n",
            "Message-ID: <m1@example.test>\r\n",
            "References: <root@example.test> <parent@example.test>\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "Line one\r\n",
            "Line two\r\n",
        )
        .as_bytes()
        .to_vec();

        let context = imap_reply_context_from_raw_mime(raw).expect("context");

        assert_eq!(context.to.len(), 1);
        assert_eq!(context.to[0].name.as_deref(), Some("Alice"));
        assert_eq!(context.to[0].email, "alice@example.test");
        assert_eq!(context.cc.len(), 2);
        assert_eq!(context.cc[0].email, "bob@example.test");
        assert_eq!(context.cc[1].email, "carol@example.test");
        assert_eq!(context.reply_subject, "Re: Hello");
        assert_eq!(context.forward_subject, "Fwd: Hello");
        assert_eq!(
            context.quoted_body.as_deref(),
            Some("> Line one\n> Line two")
        );
        assert_eq!(context.in_reply_to.as_deref(), Some("m1@example.test"));
        assert_eq!(
            context.references.as_deref(),
            Some("root@example.test parent@example.test")
        );
    }

    #[test]
    fn does_not_duplicate_reply_or_forward_subject_prefixes() {
        let raw = concat!(
            "From: Alice <alice@example.test>\r\n",
            "Subject: Re: Already replied\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "Body\r\n",
        )
        .as_bytes()
        .to_vec();

        let context = imap_reply_context_from_raw_mime(raw).expect("context");

        assert_eq!(context.reply_subject, "Re: Already replied");
        assert_eq!(context.forward_subject, "Fwd: Re: Already replied");
    }
}
