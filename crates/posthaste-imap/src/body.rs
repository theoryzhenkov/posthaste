use mail_parser::{MessageParser, MimeHeaders};
use posthaste_domain::{BlobId, FetchedBody, MessageAttachment, MessageId};

use crate::ImapAdapterError;

/// Parse a fetched raw IMAP message into Posthaste's lazy body projection.
///
/// `BODY.PEEK[]` returns bytes. The current store raw-MIME cache accepts
/// strings, so raw MIME is preserved only when the fetched message is valid
/// UTF-8. Parsed body text, HTML, and attachment metadata still come from the
/// MIME parser for non-UTF-8 messages.
///
/// @spec docs/L1-sync#body-lazy
pub fn imap_body_from_raw_mime(
    message_id: &MessageId,
    raw_mime: Vec<u8>,
) -> Result<FetchedBody, ImapAdapterError> {
    let parsed = MessageParser::default()
        .parse(&raw_mime)
        .ok_or(ImapAdapterError::ParseMessageBody)?;
    let body_html = parsed.body_html(0).map(|body| body.into_owned());
    let body_text = parsed.body_text(0).map(|body| body.into_owned());
    let attachments = parsed
        .attachments()
        .enumerate()
        .map(|(index, part)| imap_attachment_from_part(message_id, index, part))
        .collect::<Vec<_>>();
    let raw_mime = String::from_utf8(raw_mime).ok();

    Ok(FetchedBody {
        body_html,
        body_text,
        raw_mime,
        attachments,
    })
}

fn imap_attachment_from_part(
    message_id: &MessageId,
    index: usize,
    part: &mail_parser::MessagePart<'_>,
) -> MessageAttachment {
    let content_type = part.content_type();
    let content_disposition = part.content_disposition();
    let disposition = content_disposition.map(|value| value.c_type.to_string());
    let cid = part.content_id().map(str::to_string);
    let is_inline = content_disposition.is_some_and(|value| value.is_inline()) || cid.is_some();

    MessageAttachment {
        id: format!("imap-attachment-{}", index + 1),
        blob_id: imap_attachment_blob_id(message_id, index),
        part_id: Some((index + 1).to_string()),
        filename: part.attachment_name().map(str::to_string),
        mime_type: content_type
            .map(|value| match value.subtype() {
                Some(subtype) => format!("{}/{}", value.ctype(), subtype),
                None => value.ctype().to_string(),
            })
            .unwrap_or_else(|| "application/octet-stream".to_string()),
        size: part.contents().len() as i64,
        disposition,
        cid,
        is_inline,
    }
}

fn imap_attachment_blob_id(message_id: &MessageId, index: usize) -> BlobId {
    BlobId::from(format!(
        "imap:blob:{}:{}",
        hex::encode(message_id.as_str().as_bytes()),
        index + 1
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_html_and_attachment_metadata() {
        let raw = concat!(
            "From: Alice <alice@example.test>\r\n",
            "Subject: Multipart\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=\"outer\"\r\n",
            "\r\n",
            "--outer\r\n",
            "Content-Type: multipart/alternative; boundary=\"inner\"\r\n",
            "\r\n",
            "--inner\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "Plain body\r\n",
            "--inner\r\n",
            "Content-Type: text/html; charset=utf-8\r\n",
            "\r\n",
            "<p>HTML body</p>\r\n",
            "--inner--\r\n",
            "--outer\r\n",
            "Content-Type: text/plain; name=\"notes.txt\"\r\n",
            "Content-Disposition: attachment; filename=\"notes.txt\"\r\n",
            "\r\n",
            "attached text\r\n",
            "--outer--\r\n",
        );

        let body = imap_body_from_raw_mime(&MessageId::from("message-1"), raw.as_bytes().to_vec())
            .expect("body");

        assert_eq!(body.body_text.as_deref(), Some("Plain body"));
        assert_eq!(body.body_html.as_deref(), Some("<p>HTML body</p>"));
        assert_eq!(body.raw_mime.as_deref(), Some(raw));
        assert_eq!(body.attachments.len(), 1);
        assert_eq!(body.attachments[0].filename.as_deref(), Some("notes.txt"));
        assert_eq!(body.attachments[0].mime_type, "text/plain");
        assert_eq!(
            body.attachments[0].disposition.as_deref(),
            Some("attachment")
        );
        assert!(!body.attachments[0].is_inline);
        assert!(body.attachments[0]
            .blob_id
            .as_str()
            .starts_with("imap:blob:"));
    }

    #[test]
    fn non_utf8_raw_mime_keeps_parsed_body_without_raw_cache() {
        let raw = b"From: Alice <alice@example.test>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nhello \xFF\r\n".to_vec();

        let body = imap_body_from_raw_mime(&MessageId::from("message-1"), raw).expect("body");

        assert!(body.body_text.is_some());
        assert_eq!(body.raw_mime, None);
    }
}
