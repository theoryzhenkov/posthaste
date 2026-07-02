//! Message + conversation detail reads, and the single resource-serve
//! transform chokepoint (body sanitization + the shared byte-response builder).
//! The handlers live in `detail::handlers`; the transform stays here so the
//! `resource_boundary` guard finds one sanitize + one builder call site.

use super::*;

pub(crate) mod handlers;

pub use handlers::{get_conversation, get_message, get_message_attachment, get_message_body};

/// Resolve a lazy message resource and serve it: fetch raw bytes from the
/// runtime, apply the per-kind transform policy, and build the byte response.
/// Every resource byte endpoint (attachment, body) goes through this one path.
pub(crate) async fn serve_message_resource(
    state: &Arc<AppState>,
    source_id: String,
    message_id: String,
    kind: MessageResourceKind,
    download: bool,
) -> Result<Response, ApiError> {
    let resource = state
        .runtime
        .get_message_resource(
            RuntimeCaller::api(),
            AccountId(source_id.clone()),
            MessageId(message_id.clone()),
            kind.clone(),
        )
        .await
        .map_err(ApiError::from_runtime_error)?;
    let resource = apply_resource_transform(&source_id, &message_id, &kind, resource);
    serve_resource_response(resource, download)
}

/// The per-kind serve policy — the single place a resource's bytes are
/// transformed. Body HTML is sanitized then has its inline `cid:` URLs rewritten
/// (byte-identical to what the detail endpoint used to do); every other resource
/// is served verbatim.
fn apply_resource_transform(
    source_id: &str,
    message_id: &str,
    kind: &MessageResourceKind,
    resource: RuntimeResourceBytes,
) -> RuntimeResourceBytes {
    match kind {
        MessageResourceKind::BodyHtml => {
            let html = String::from_utf8_lossy(&resource.bytes);
            let sanitized = sanitize::sanitize_email_html(&html);
            let rewritten = rewrite_inline_attachment_urls(
                &sanitized,
                source_id,
                message_id,
                &resource.inline_attachments,
            );
            RuntimeResourceBytes {
                bytes: rewritten.into_bytes(),
                ..resource
            }
        }
        MessageResourceKind::Attachment(_) | MessageResourceKind::BodyText => resource,
    }
}

/// Build the HTTP response for a resolved lazy message resource: content type,
/// inline/attachment disposition, and the shared cache policy. Every resource
/// byte response (attachment, body) goes through this one builder.
pub(crate) fn serve_resource_response(
    resource: RuntimeResourceBytes,
    download: bool,
) -> Result<Response, ApiError> {
    let disposition_kind = if download { "attachment" } else { "inline" };
    let filename = resource.filename.as_deref().unwrap_or("resource");
    let content_disposition = format!(
        "{disposition_kind}; filename=\"{}\"",
        escape_content_disposition_filename(filename)
    );

    let mut response = Response::new(Body::from(resource.bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(resource.content_type.as_str())
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&content_disposition)
            .map_err(|_| internal_error("invalid content disposition header".to_string()))?,
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=300"),
    );
    Ok(response)
}

fn rewrite_inline_attachment_urls(
    html: &str,
    source_id: &str,
    message_id: &str,
    attachments: &[MessageAttachment],
) -> String {
    let mut rewritten = html.to_string();
    for attachment in attachments {
        if !attachment.is_inline {
            continue;
        }
        let Some(cid) = attachment.cid.as_deref() else {
            continue;
        };
        let normalized = cid.trim().trim_start_matches('<').trim_end_matches('>');
        let url = format!(
            "/v1/sources/{source_id}/messages/{message_id}/attachments/{}",
            attachment.id
        );
        rewritten = rewritten.replace(&format!("cid:{normalized}"), &url);
        rewritten = rewritten.replace(&format!("cid:<{normalized}>"), &url);
    }
    rewritten
}

fn escape_content_disposition_filename(filename: &str) -> String {
    filename.replace('\\', "_").replace('"', "'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_domain_model::BlobId;

    fn inline_attachment(id: &str, cid: &str) -> MessageAttachment {
        MessageAttachment {
            id: id.to_string(),
            blob_id: BlobId::from("blob-1"),
            part_id: None,
            filename: None,
            mime_type: "image/png".to_string(),
            size: 0,
            disposition: Some("inline".to_string()),
            cid: Some(cid.to_string()),
            is_inline: true,
        }
    }

    // The body-html serve transform must reproduce the old detail behavior
    // exactly: sanitize first, then rewrite inline `cid:` URLs. This is the
    // security-critical path (XSS surface), so it is asserted directly.
    #[test]
    fn body_html_transform_sanitizes_then_rewrites_cid_urls() {
        let resource = RuntimeResourceBytes {
            bytes: br#"<script>alert(1)</script><img src="cid:img1"><p>hi</p>"#.to_vec(),
            content_type: "text/html; charset=utf-8".to_string(),
            filename: None,
            inline_attachments: vec![inline_attachment("att-1", "img1")],
        };
        let out = apply_resource_transform("acct", "msg", &MessageResourceKind::BodyHtml, resource);
        let html = String::from_utf8(out.bytes).expect("utf8");
        assert!(
            !html.contains("<script>"),
            "script must be sanitized out: {html}"
        );
        assert!(
            html.contains("/v1/sources/acct/messages/msg/attachments/att-1"),
            "cid must be rewritten to the attachment URL: {html}"
        );
        assert!(!html.contains("cid:img1"), "raw cid must be gone: {html}");
    }

    #[test]
    fn non_body_resources_are_served_verbatim() {
        let raw = b"\x00\x01raw-bytes<script>".to_vec();
        let resource = RuntimeResourceBytes {
            bytes: raw.clone(),
            content_type: "application/octet-stream".to_string(),
            filename: Some("f.bin".to_string()),
            inline_attachments: Vec::new(),
        };
        let out = apply_resource_transform(
            "a",
            "m",
            &MessageResourceKind::Attachment("x".to_string()),
            resource,
        );
        assert_eq!(out.bytes, raw);
    }
}
