use super::*;

#[test]
fn send_message_rejects_missing_to_recipient() {
    let error = validate_send_message_request(&SendMessageRequest {
        from: None,
        to: Vec::new(),
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: "Hello".to_string(),
        body: "Body".to_string(),
        in_reply_to: None,
        references: None,
        attachments: Vec::new(),
        draft_id: None,
    })
    .expect_err("empty To should be rejected");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.body.code, ApiErrorCode::InvalidCompose);
}

#[test]
fn send_message_rejects_invalid_attachment_base64() {
    let error = validate_send_message_request(&SendMessageRequest {
        from: None,
        to: vec![Recipient {
            name: None,
            email: "to@example.test".to_string(),
        }],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: "Hello".to_string(),
        body: "Body".to_string(),
        in_reply_to: None,
        references: None,
        attachments: vec![posthaste_domain::SendMessageAttachment {
            filename: "notes.txt".to_string(),
            mime_type: "text/plain".to_string(),
            content_base64: "not base64".to_string(),
        }],
        draft_id: None,
    })
    .expect_err("invalid attachment should be rejected");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.body.code, ApiErrorCode::InvalidCompose);
}

#[test]
fn send_message_rejects_too_many_attachments() {
    let error = validate_send_message_request(&SendMessageRequest {
        from: None,
        to: vec![Recipient {
            name: None,
            email: "to@example.test".to_string(),
        }],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: "Hello".to_string(),
        body: "Body".to_string(),
        in_reply_to: None,
        references: None,
        attachments: (0..=MAX_SEND_ATTACHMENTS)
            .map(|index| posthaste_domain::SendMessageAttachment {
                filename: format!("notes-{index}.txt"),
                mime_type: "text/plain".to_string(),
                content_base64: "aGVsbG8=".to_string(),
            })
            .collect(),
        draft_id: None,
    })
    .expect_err("too many attachments should be rejected");

    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.body.code, ApiErrorCode::InvalidCompose);
}
