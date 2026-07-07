use posthaste_domain_model::{BlobId, FetchedBody};

pub(super) fn empty_body() -> FetchedBody {
    FetchedBody {
        body_html: None,
        body_text: None,
        raw_mime: None,
        attachments: Vec::new(),
        list_unsubscribe: None,
    }
}

pub(super) fn parity_body() -> FetchedBody {
    FetchedBody {
        body_html: Some("<p>HTML body</p>".to_string()),
        body_text: Some("Plain body".to_string()),
        raw_mime: None,
        attachments: vec![posthaste_domain_model::MessageAttachment {
            id: "attachment-1".to_string(),
            blob_id: BlobId::from("jmap-blob-1"),
            part_id: Some("1".to_string()),
            filename: Some("notes.txt".to_string()),
            mime_type: "text/plain".to_string(),
            size: 13,
            disposition: Some("attachment".to_string()),
            cid: None,
            is_inline: false,
        }],
        list_unsubscribe: None,
    }
}

pub(super) fn parity_raw_mime() -> Vec<u8> {
    concat!(
        "From: Alice <alice@example.test>\r\n",
        "Subject: Parity subject\r\n",
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
    )
    .as_bytes()
    .to_vec()
}

pub(super) fn parity_attachment_blob() -> Vec<u8> {
    b"attached text".to_vec()
}
