use imap_client::imap_types::fetch::{
    MacroOrMessageDataItemNames, MessageDataItem, MessageDataItemName,
};
use mail_parser::{MessageParser, MimeHeaders};
use posthaste_domain::{BlobId, FetchedBody, ImapMessageLocation, MessageAttachment, MessageId};

use crate::discovery::connect_authenticated_client;
use crate::{selected_mailbox_from_examine, ImapAdapterError, ImapConnectionConfig};

/// Fetch a full raw IMAP message without marking it read, then parse it into
/// Posthaste's lazy body projection.
///
/// @spec docs/L1-sync#body-lazy
pub async fn fetch_message_body_by_location(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    location: &ImapMessageLocation,
) -> Result<FetchedBody, ImapAdapterError> {
    let raw_mime = fetch_raw_message_by_location(config, mailbox_name, location).await?;
    imap_body_from_raw_mime(&location.message_id, raw_mime)
}

/// Fetch a full raw IMAP message without marking it read.
///
/// The raw fetch is shared by lazy body projection and attachment download so
/// both paths validate the same `(mailbox, UIDVALIDITY, UID)` identity before
/// trusting `BODY.PEEK[]` bytes.
///
/// @spec docs/L1-sync#body-lazy
pub async fn fetch_raw_message_by_location(
    config: &ImapConnectionConfig,
    mailbox_name: &str,
    location: &ImapMessageLocation,
) -> Result<Vec<u8>, ImapAdapterError> {
    let mut client = connect_authenticated_client(config).await?;
    let selected =
        selected_mailbox_from_examine(mailbox_name, client.examine(mailbox_name).await?)?;
    if selected.uid_validity != location.uid_validity {
        return Err(ImapAdapterError::UidValidityMismatch {
            mailbox_name: mailbox_name.to_string(),
            expected: location.uid_validity.0,
            actual: selected.uid_validity.0,
        });
    }

    let uid = std::num::NonZeroU32::new(location.uid.0)
        .ok_or_else(|| ImapAdapterError::InvalidUidSequence("UID 0".to_string()))?;
    let items = client
        .uid_fetch_first(uid, body_fetch_item_names())
        .await
        .map_err(ImapAdapterError::from)?;

    raw_mime_from_items(location, items)
}

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

fn body_fetch_item_names() -> MacroOrMessageDataItemNames<'static> {
    MacroOrMessageDataItemNames::MessageDataItemNames(vec![
        MessageDataItemName::Uid,
        MessageDataItemName::BodyExt {
            section: None,
            partial: None,
            peek: true,
        },
    ])
}

pub fn fetched_body_from_items(
    message_id: &MessageId,
    location: &ImapMessageLocation,
    items: impl IntoIterator<Item = MessageDataItem<'static>>,
) -> Result<FetchedBody, ImapAdapterError> {
    let raw_mime = raw_mime_from_items(location, items)?;
    imap_body_from_raw_mime(message_id, raw_mime)
}

pub fn raw_mime_from_items(
    location: &ImapMessageLocation,
    items: impl IntoIterator<Item = MessageDataItem<'static>>,
) -> Result<Vec<u8>, ImapAdapterError> {
    let mut uid = None;
    let mut raw_mime = None;

    for item in items {
        match item {
            MessageDataItem::Uid(next_uid) => {
                uid = Some(next_uid.get());
            }
            MessageDataItem::BodyExt {
                section: None,
                origin: None,
                data,
            } => {
                raw_mime = data.into_option().map(|bytes| bytes.into_owned());
            }
            _ => {}
        }
    }

    let uid = uid.ok_or(ImapAdapterError::MissingFetchData("UID"))?;
    if uid != location.uid.0 {
        return Err(ImapAdapterError::MissingFetchData("matching UID"));
    }
    raw_mime.ok_or(ImapAdapterError::MissingFetchData("BODY.PEEK[]"))
}

pub fn parse_imap_attachment_blob_id(
    blob_id: &BlobId,
) -> Result<(MessageId, usize), ImapAdapterError> {
    let mut parts = blob_id.as_str().split(':');
    let Some("imap") = parts.next() else {
        return Err(ImapAdapterError::InvalidBlobId(blob_id.to_string()));
    };
    let Some("blob") = parts.next() else {
        return Err(ImapAdapterError::InvalidBlobId(blob_id.to_string()));
    };
    let Some(message_id_hex) = parts.next() else {
        return Err(ImapAdapterError::InvalidBlobId(blob_id.to_string()));
    };
    let Some(attachment_index) = parts.next() else {
        return Err(ImapAdapterError::InvalidBlobId(blob_id.to_string()));
    };
    if parts.next().is_some() {
        return Err(ImapAdapterError::InvalidBlobId(blob_id.to_string()));
    }

    let message_id_bytes = hex::decode(message_id_hex)
        .map_err(|_| ImapAdapterError::InvalidBlobId(blob_id.to_string()))?;
    let message_id = String::from_utf8(message_id_bytes)
        .map_err(|_| ImapAdapterError::InvalidBlobId(blob_id.to_string()))?;
    let attachment_index = attachment_index
        .parse::<usize>()
        .map_err(|_| ImapAdapterError::InvalidBlobId(blob_id.to_string()))?;
    if attachment_index == 0 {
        return Err(ImapAdapterError::InvalidBlobId(blob_id.to_string()));
    }

    Ok((MessageId::from(message_id), attachment_index))
}

pub fn imap_attachment_bytes_from_raw_mime(
    blob_id: &BlobId,
    raw_mime: Vec<u8>,
) -> Result<Vec<u8>, ImapAdapterError> {
    let (message_id, attachment_index) = parse_imap_attachment_blob_id(blob_id)?;
    let parsed = MessageParser::default()
        .parse(&raw_mime)
        .ok_or(ImapAdapterError::ParseMessageBody)?;
    let attachment = parsed
        .attachment((attachment_index - 1) as u32)
        .ok_or_else(|| ImapAdapterError::MissingAttachment {
            message_id: message_id.to_string(),
            attachment_index,
        })?;

    Ok(attachment.contents().to_vec())
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
    use std::num::NonZeroU32;

    use imap_client::imap_types::core::NString;
    use posthaste_domain::{ImapUid, ImapUidValidity, MailboxId};

    use super::*;

    fn location() -> ImapMessageLocation {
        ImapMessageLocation {
            message_id: MessageId::from("message-1"),
            mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
            uid_validity: ImapUidValidity(9),
            uid: ImapUid(42),
            modseq: None,
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        }
    }

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

    #[test]
    fn fetched_body_extracts_body_peek_without_seen_side_effect_item() {
        let raw = b"From: Alice <alice@example.test>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nPlain body\r\n";
        let body = fetched_body_from_items(
            &MessageId::from("message-1"),
            &location(),
            [
                MessageDataItem::Uid(NonZeroU32::new(42).expect("uid")),
                MessageDataItem::BodyExt {
                    section: None,
                    origin: None,
                    data: NString::try_from(raw.as_slice()).expect("raw nstring"),
                },
            ],
        )
        .expect("body");

        assert_eq!(body.body_text.as_deref(), Some("Plain body\r\n"));
    }

    #[test]
    fn fetched_body_rejects_missing_body_peek() {
        let error = fetched_body_from_items(
            &MessageId::from("message-1"),
            &location(),
            [MessageDataItem::Uid(NonZeroU32::new(42).expect("uid"))],
        )
        .expect_err("body is required");

        assert!(matches!(
            error,
            ImapAdapterError::MissingFetchData("BODY.PEEK[]")
        ));
    }

    #[test]
    fn attachment_blob_id_round_trips_to_decoded_attachment_bytes() {
        let message_id = MessageId::from("message-1");
        let raw = concat!(
            "From: Alice <alice@example.test>\r\n",
            "Subject: Attachment\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=\"outer\"\r\n",
            "\r\n",
            "--outer\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "\r\n",
            "Plain body\r\n",
            "--outer\r\n",
            "Content-Type: application/octet-stream; name=\"notes.bin\"\r\n",
            "Content-Disposition: attachment; filename=\"notes.bin\"\r\n",
            "Content-Transfer-Encoding: base64\r\n",
            "\r\n",
            "aGVsbG8gYXR0YWNobWVudA==\r\n",
            "--outer--\r\n",
        )
        .as_bytes()
        .to_vec();
        let body = imap_body_from_raw_mime(&message_id, raw.clone()).expect("body");

        let (parsed_message_id, parsed_attachment_index) =
            parse_imap_attachment_blob_id(&body.attachments[0].blob_id).expect("blob id");
        let attachment_bytes =
            imap_attachment_bytes_from_raw_mime(&body.attachments[0].blob_id, raw)
                .expect("attachment");

        assert_eq!(parsed_message_id, message_id);
        assert_eq!(parsed_attachment_index, 1);
        assert_eq!(attachment_bytes, b"hello attachment");
    }

    #[test]
    fn attachment_blob_id_rejects_unknown_format() {
        let error = parse_imap_attachment_blob_id(&BlobId::from("jmap-blob-1"))
            .expect_err("JMAP blob id is not an IMAP blob id");

        assert!(matches!(error, ImapAdapterError::InvalidBlobId(_)));
    }

    #[test]
    fn attachment_download_reports_missing_attachment_index() {
        let message_id = MessageId::from("message-1");
        let raw = b"From: Alice <alice@example.test>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nPlain body\r\n".to_vec();
        let blob_id = BlobId::from(format!(
            "imap:blob:{}:2",
            hex::encode(message_id.as_str().as_bytes())
        ));

        let error = imap_attachment_bytes_from_raw_mime(&blob_id, raw)
            .expect_err("attachment index should not exist");

        assert!(matches!(
            error,
            ImapAdapterError::MissingAttachment {
                message_id: ref actual_message_id,
                attachment_index: 2,
            } if actual_message_id == "message-1"
        ));
    }
}
