use jmap_client::email;
use posthaste_domain::{
    synthesize_plain_text_raw_mime, BlobId, FetchedBody, GatewayError, MessageAttachment, MessageId,
};

use crate::live::{map_gateway_error, required_method_response, LiveJmapGateway};

/// Lazily fetch the body content of a single message via `Email/get`.
///
/// Bodies are not synced during metadata sync; they are fetched on first
/// view and cached locally.
///
/// @spec docs/L1-sync#sync-granularity
/// @spec docs/L1-jmap#methods-used
pub(crate) async fn fetch_message_body(
    gateway: &LiveJmapGateway,
    message_id: &MessageId,
) -> Result<FetchedBody, GatewayError> {
    let mut request = gateway.client().build();
    let get_request = request.get_email().ids([message_id.as_str()]).properties([
        email::Property::Id,
        email::Property::Attachments,
        email::Property::BodyValues,
        email::Property::HtmlBody,
        email::Property::TextBody,
    ]);
    get_request
        .arguments()
        .body_properties([
            email::BodyProperty::BlobId,
            email::BodyProperty::Cid,
            email::BodyProperty::Disposition,
            email::BodyProperty::Name,
            email::BodyProperty::PartId,
            email::BodyProperty::Size,
            email::BodyProperty::Type,
        ])
        .fetch_all_body_values(true);

    let mut response = gateway.send_request(request).await?;
    let mut emails = required_method_response(response.pop_method_response(), "Email/get")?
        .unwrap_get_email()
        .map_err(map_gateway_error)?
        .take_list();
    let email = emails
        .pop()
        .ok_or_else(|| GatewayError::Rejected("message not found".to_string()))?;

    let body_html = email.html_body().and_then(|parts| {
        parts
            .first()
            .and_then(|part| part.part_id())
            .and_then(|part_id| email.body_value(part_id))
            .map(|value| value.value().to_string())
    });
    let body_text = email.text_body().and_then(|parts| {
        parts
            .first()
            .and_then(|part| part.part_id())
            .and_then(|part_id| email.body_value(part_id))
            .map(|value| value.value().to_string())
    });
    let from_header = email
        .from()
        .and_then(|addresses| addresses.first())
        .map(|address| address.email().to_string())
        .unwrap_or_else(|| "unknown@example.invalid".to_string());
    let raw_mime = synthesize_plain_text_raw_mime(
        from_header.as_str(),
        email.subject().unwrap_or("(no subject)"),
        body_text.as_deref(),
    );
    let attachments = email
        .attachments()
        .map(|parts| {
            parts
                .iter()
                .enumerate()
                .filter_map(|(index, part)| attachment_from_part(index, part))
                .collect()
        })
        .unwrap_or_default();

    Ok(FetchedBody {
        body_html,
        body_text,
        raw_mime: Some(raw_mime),
        attachments,
    })
}

pub(crate) async fn download_blob(
    gateway: &LiveJmapGateway,
    blob_id: &BlobId,
) -> Result<Vec<u8>, GatewayError> {
    gateway
        .client()
        .download(blob_id.as_str())
        .await
        .map_err(map_gateway_error)
}

fn attachment_from_part(index: usize, part: &email::EmailBodyPart) -> Option<MessageAttachment> {
    let blob_id = BlobId::from(part.blob_id()?.to_string());
    let disposition = part.content_disposition().map(str::to_string);
    let cid = part.content_id().map(str::to_string);
    let is_inline = disposition.as_deref() == Some("inline") || cid.is_some();
    Some(MessageAttachment {
        id: format!("attachment-{}", index + 1),
        blob_id,
        part_id: part.part_id().map(str::to_string),
        filename: part.name().map(str::to_string),
        mime_type: part
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string(),
        size: part.size() as i64,
        disposition,
        cid,
        is_inline,
    })
}
