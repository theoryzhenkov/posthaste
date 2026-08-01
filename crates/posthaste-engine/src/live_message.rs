use jmap_client::email;
use posthaste_domain_model::{
    synthesize_plain_text_raw_mime_with_recipients, BlobId, FetchedBody, GatewayError,
    MessageAttachment, MessageId, MessageRecord,
};
use posthaste_provider_call::{CallClass, HttpRequestSpec};

use crate::live::{
    map_gateway_error, map_provider_error, required_method_response, LiveJmapGateway,
};

/// Read a single message's authoritative metadata record via `Email/get` — the
/// `get` half of a mutation's set+get. Returns the provider's current state of
/// the message after the change (including any concurrent external change),
/// which drives optimistic settlement at the runtime.
///
/// @spec docs/eph/DESIGN-L2-optimistic-projection#3-the-runtime-write-through-mechanics
/// @spec docs/L1-jmap#methods-used
pub(crate) async fn fetch_message_record(
    gateway: &LiveJmapGateway,
    message_id: &MessageId,
) -> Result<MessageRecord, GatewayError> {
    let mut request = gateway.client().build();
    request
        .get_email()
        .ids([message_id.as_str()])
        .properties(crate::sync::email_metadata_properties());
    let mut response = gateway.send_request(request).await?;
    let mut emails = required_method_response(response.pop_method_response(), "Email/get")?
        .unwrap_get_email()
        .map_err(map_gateway_error)?
        .take_list();
    let email = emails
        .pop()
        .ok_or_else(|| GatewayError::Rejected("message not found after mutation".to_string()))?;
    Ok(crate::conversions::to_message_record(&email))
}

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
        email::Property::Bcc,
        email::Property::BodyValues,
        email::Property::Cc,
        email::Property::From,
        email::Property::HtmlBody,
        email::Property::Subject,
        email::Property::TextBody,
        email::Property::To,
        // Completes the recipient set the body fetch already asked for (Cc and
        // Bcc were here for draft-resume MIME synthesis), so it doubles as the
        // old-mail backfill source for the recipient columns.
        email::Property::ReplyTo,
        // Unsubscribe headers ride along on the body fetch so messages synced
        // before the `list_unsubscribe` column existed are backfilled at
        // message-open (JMAP serves no raw RFC822 here, so headers must be
        // requested explicitly).
        email::Property::Header(email::Header::as_raw("List-Unsubscribe", false)),
        email::Property::Header(email::Header::as_raw("List-Unsubscribe-Post", false)),
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

    let mut response = send_body_request(gateway, request).await?;
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
        .map(|address| address.email().to_string());
    let to = email
        .to()
        .map(crate::compose::addresses_to_recipients)
        .unwrap_or_default();
    let cc = email
        .cc()
        .map(crate::compose::addresses_to_recipients)
        .unwrap_or_default();
    let bcc = email
        .bcc()
        .map(crate::compose::addresses_to_recipients)
        .unwrap_or_default();
    let raw_mime = synthesize_plain_text_raw_mime_with_recipients(
        from_header.as_deref(),
        &to,
        &cc,
        &bcc,
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
        list_unsubscribe: crate::conversions::list_unsubscribe_from_email(&email),
        cc: crate::conversions::recipients_from(email.cc()),
        bcc: crate::conversions::recipients_from(email.bcc()),
        reply_to: crate::conversions::recipients_from(email.reply_to()),
    })
}

/// Dispatch a body-shaped `Email/get` (full `bodyValues`) with a *blob-class*
/// deadline instead of the metadata total.
///
/// A body fetch can carry megabytes of inline `bodyValues`, so the metadata
/// path's fixed 30 s total (jmap-client's own HTTP client timeout) has the F2
/// shape: it deterministically fails a large-but-progressing body on a slow
/// link, while a genuinely stalled read still gets to sit out the full total.
/// Routing the HTTP dispatch through the provider-call envelope as
/// `CallClass::Blob` gives it the between-chunks byte-progress stall guard
/// (M31/M34, `BLOB_STALL`) and no total — parity with `download_blob` below.
///
/// WebSocket, when connected, is preferred unchanged: its liveness is owned by
/// the keepalive/read-deadline machinery (D88), and the cache worker's
/// batch-level deadline bounds a hung WS reply at the call site. Falls back to
/// the plain metadata-class send if the shared executor failed to build.
async fn send_body_request(
    gateway: &LiveJmapGateway,
    request: jmap_client::core::request::Request<'_>,
) -> Result<
    jmap_client::core::response::Response<jmap_client::core::response::TaggedMethodResponse>,
    GatewayError,
> {
    if let Some(ws) = gateway.ws() {
        if ws.is_connected().await {
            return ws.send(request).await;
        }
    }
    if let Some(executor) = gateway.executor() {
        // Serializing our own request is an internal codec fault, not a
        // network error (mirrors the raw JMAP POST path in live_mutation).
        let body = serde_json::to_vec(&request)
            .map_err(|error| GatewayError::Internal(error.to_string()))?;
        let spec = HttpRequestSpec::post(
            gateway.client().session().api_url(),
            gateway.client().headers().clone(),
            body,
        );
        let bytes = executor
            .execute(gateway.account_key(), CallClass::Blob, spec)
            .await
            .map_err(map_provider_error)?
            .body;
        // The bytes arrived; a decode failure is a codec fault, not transient.
        return serde_json::from_slice(&bytes)
            .map_err(|error| GatewayError::Internal(error.to_string()));
    }
    gateway.send_request(request).await
}

pub(crate) async fn download_blob(
    gateway: &LiveJmapGateway,
    blob_id: &BlobId,
) -> Result<Vec<u8>, GatewayError> {
    // Route blob downloads through the provider-call envelope as a *blob-class*
    // call (M31): no total timeout, a between-chunks stall read-deadline instead,
    // so a large-but-progressing attachment on a slow link completes rather than
    // deterministically timing out at 10 s (F2). Falls back to jmap-client's own
    // `download()` only if the shared executor failed to build.
    if let Some(executor) = gateway.executor() {
        let url = build_download_url(gateway, blob_id);
        let mut headers = gateway.client().headers().clone();
        headers.remove(reqwest::header::CONTENT_TYPE);
        let spec = HttpRequestSpec::get(url, headers);
        return executor
            .execute(gateway.account_key(), CallClass::Blob, spec)
            .await
            .map(|response| response.body)
            .map_err(map_provider_error);
    }
    gateway
        .client()
        .download(blob_id.as_str())
        .await
        .map_err(map_gateway_error)
}

/// Build the JMAP blob download URL from the session's URI template, matching
/// jmap-client's own substitution (`name`/`type` default to the same placeholder
/// values its `download()` uses).
fn build_download_url(gateway: &LiveJmapGateway, blob_id: &BlobId) -> String {
    gateway
        .client()
        .session()
        .download_url()
        .replace("{accountId}", gateway.server_account_id())
        .replace("{blobId}", blob_id.as_str())
        .replace("{name}", "none")
        .replace("{type}", "application/octet-stream")
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
