use imap_client::imap_types::fetch::{
    MacroOrMessageDataItemNames, MessageDataItem, MessageDataItemName,
};
use mail_parser::{MessageParser, MimeHeaders};
use posthaste_domain_model::{BlobId, MessageId};
use posthaste_domain_model::{FetchedBody, ImapMessageLocation, MessageAttachment};

use imap_client::client::tokio::Client as ImapClient;

use crate::{selected_mailbox_from_examine, ImapAdapterError};

/// Fetch a full raw IMAP message without marking it read, then parse it into
/// Posthaste's lazy body projection.
///
/// @spec docs/L1-sync#body-lazy
pub(crate) async fn fetch_message_body_by_location(
    client: &mut ImapClient,
    mailbox_name: &str,
    location: &ImapMessageLocation,
) -> Result<FetchedBody, ImapAdapterError> {
    let raw_mime = fetch_raw_message_by_location(client, mailbox_name, location).await?;
    imap_body_from_raw_mime(&location.message_id, raw_mime)
}

/// Fetch a full raw IMAP message without marking it read.
///
/// The raw fetch is shared by lazy body projection and attachment download so
/// both paths validate the same `(mailbox, UIDVALIDITY, UID)` identity before
/// trusting `BODY.PEEK[]` bytes.
///
/// Deadline shape: each round trip is bounded by the 60 s per-op envelope
/// (`with_deadline` / `IMAP_OP_TIMEOUT_MS`). A byte-progress *stall* guard
/// (parity with the JMAP blob class, M31/M34) is not implementable at this
/// seam: `imap-client`'s task API resolves a whole `UID FETCH` response — one
/// message body arrives as a single literal parsed only when complete — and
/// exposes no read-progress or transport hook, so per-op wall clock is the
/// tightest available cut without forking the client. The cache worker's
/// batch-level deadline (`BODY_CACHE_BATCH_BUDGET`) additionally bounds a
/// stalled fetch on the background cache path.
///
/// @spec docs/L1-sync#body-lazy
pub(crate) async fn fetch_raw_message_by_location(
    client: &mut ImapClient,
    mailbox_name: &str,
    location: &ImapMessageLocation,
) -> Result<Vec<u8>, ImapAdapterError> {
    let selected = selected_mailbox_from_examine(
        mailbox_name,
        crate::timeout::with_deadline("examine", client.examine(mailbox_name)).await?,
    )?;
    if selected.uid_validity != location.uid_validity {
        return Err(ImapAdapterError::UidValidityMismatch {
            mailbox_name: mailbox_name.to_string(),
            expected: location.uid_validity.0,
            actual: selected.uid_validity.0,
        });
    }

    let uid = std::num::NonZeroU32::new(location.uid.0)
        .ok_or_else(|| ImapAdapterError::InvalidUidSequence("UID 0".to_string()))?;
    let items = crate::timeout::with_deadline(
        "uid_fetch_first",
        client.uid_fetch_first(uid, body_fetch_item_names()),
    )
    .await?;

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
    // Old-mail backfill: the full raw message carries the headers, so a body
    // fetch re-extracts the unsubscribe targets and the recipient set for rows
    // synced before those columns existed. The store's offline re-derive pass
    // runs this same derivation over the `.eml` this fetch is about to cache,
    // for the mail whose body was cached before the columns existed and so
    // never reaches this path again.
    let derived = posthaste_domain_service::derive_message_metadata_from_parsed(&parsed);
    let raw_mime = String::from_utf8(raw_mime).ok();

    Ok(FetchedBody {
        body_html,
        body_text,
        raw_mime,
        attachments,
        list_unsubscribe: derived.list_unsubscribe,
        cc: derived.cc,
        bcc: derived.bcc,
        reply_to: derived.reply_to,
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
mod tests;
