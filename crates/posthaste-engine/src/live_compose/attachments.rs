use base64::Engine;
use posthaste_domain_model::{GatewayError, SendMessageAttachment};

use crate::live::{map_gateway_error, LiveJmapGateway};

pub(crate) struct UploadedSendAttachment {
    pub filename: String,
    pub mime_type: String,
    pub blob_id: String,
}

pub(crate) async fn upload_send_attachments(
    gateway: &LiveJmapGateway,
    attachments: &[SendMessageAttachment],
) -> Result<Vec<UploadedSendAttachment>, GatewayError> {
    let mut uploaded = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        let bytes = decode_attachment_bytes(attachment)?;
        // Blobs are per-account in JMAP, and `Email/set` references them under the
        // client's default (server) account id. The upload must target that same
        // account id — not the Posthaste-internal AccountId — or `Email/set`
        // rejects the blob with `blobNotFound`.
        let response = gateway
            .client()
            .upload(
                Some(gateway.server_account_id()),
                bytes,
                Some(normalized_attachment_mime_type(attachment).as_str()),
            )
            .await
            .map_err(map_gateway_error)?;
        uploaded.push(UploadedSendAttachment {
            filename: attachment.filename.trim().to_string(),
            mime_type: normalized_attachment_mime_type(attachment),
            blob_id: response.blob_id().to_string(),
        });
    }
    Ok(uploaded)
}

fn decode_attachment_bytes(attachment: &SendMessageAttachment) -> Result<Vec<u8>, GatewayError> {
    base64::engine::general_purpose::STANDARD
        .decode(attachment.content_base64.trim())
        .map_err(|_| {
            GatewayError::Rejected(format!(
                "attachment {} is not valid base64",
                attachment.filename
            ))
        })
}

fn normalized_attachment_mime_type(attachment: &SendMessageAttachment) -> String {
    let mime_type = attachment.mime_type.trim();
    if mime_type.is_empty() {
        "application/octet-stream".to_string()
    } else {
        mime_type.to_string()
    }
}
