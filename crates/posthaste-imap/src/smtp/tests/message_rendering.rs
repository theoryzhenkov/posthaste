use posthaste_domain_service::{ProviderAuthKind, ProviderHint, SendMessageRequest};

use crate::ImapAdapterError;

use super::*;

#[test]
fn config_from_account_settings_requires_smtp_settings() {
    let mut account = test_account(None, vec!["alice@example.test"], "alice-login");
    account.transport.smtp = None;

    let error = SmtpConnectionConfig::from_account_settings(&account, "secret".to_string())
        .expect_err("SMTP settings should be required");

    assert!(matches!(error, ImapAdapterError::MissingSmtpTransport));
}

#[test]
fn builds_multipart_message_with_threading_headers_and_hidden_bcc() {
    let config = test_config();
    let request = SendMessageRequest {
        from: None,
        to: vec![recipient(Some("Bob"), "bob@example.test")],
        cc: vec![recipient(None, "carol@example.test")],
        bcc: vec![recipient(Some("Dana"), "dana@example.test")],
        subject: "Status".to_string(),
        body: "Hello **world**".to_string(),
        in_reply_to: Some("original@example.test".to_string()),
        references: Some("root@example.test original@example.test".to_string()),
        ..Default::default()
    };

    let message = build_smtp_message(&config, &request).expect("SMTP message");
    let formatted = String::from_utf8(message.formatted()).expect("message is UTF-8");

    assert!(formatted.contains("From: alice <alice@example.test>"));
    assert!(formatted.contains("To: Bob <bob@example.test>"));
    assert!(formatted.contains("Cc: carol@example.test"));
    assert!(formatted.contains("Subject: Status"));
    assert!(formatted.contains("Message-ID: <"));
    assert!(formatted.contains("Date: "));
    assert!(formatted.contains("In-Reply-To: <original@example.test>"));
    assert!(formatted.contains("References: <root@example.test> <original@example.test>"));
    assert!(formatted.contains("Content-Type: multipart/alternative;"));
    assert!(formatted.contains("Content-Type: text/plain"));
    assert!(formatted.contains("Content-Type: text/html"));
    assert!(formatted.contains("Hello **world**"));
    assert!(render_smtp_markdown(&request.body).contains("<strong>world</strong>"));
    assert!(!formatted.contains("Bcc:"));
    assert!(!formatted.contains("dana@example.test"));
}

#[test]
fn builds_multipart_mixed_message_with_attachments() {
    let config = test_config();
    let request = SendMessageRequest {
        from: None,
        to: vec![recipient(Some("Bob"), "bob@example.test")],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: "Files".to_string(),
        body: "See attached.".to_string(),
        in_reply_to: None,
        references: None,
        attachments: vec![posthaste_domain_service::SendMessageAttachment {
            filename: "notes.txt".to_string(),
            mime_type: "text/plain".to_string(),
            content_base64: "aGVsbG8gYXR0YWNobWVudA==".to_string(),
        }],
        ..Default::default()
    };

    let message = build_smtp_message(&config, &request).expect("SMTP message");
    let formatted = String::from_utf8(message.formatted()).expect("message is UTF-8");

    assert!(formatted.contains("Content-Type: multipart/mixed;"));
    assert!(formatted.contains("Content-Type: multipart/alternative;"));
    assert!(formatted.contains("Content-Type: text/plain"));
    assert!(formatted.contains("Content-Disposition: attachment; filename=\"notes.txt\""));
    assert!(formatted.contains("hello attachment"));
}

#[test]
fn render_smtp_markdown_excludes_raw_html_and_markdown_images() {
    let rendered = render_smtp_markdown(
        "<script>alert(1)</script>\n\n![pixel](https://example.test/pixel.png)",
    );

    assert!(rendered.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!rendered.contains("<script>"));
    assert!(!rendered.contains("<img"));
    assert!(!rendered.contains("https://example.test/pixel.png"));
    assert!(rendered.contains("pixel"));
}

#[test]
fn builds_message_with_requested_from_identity() {
    let config = test_config();
    let request = SendMessageRequest {
        from: Some(recipient(Some("Catch All"), "catch@example.test")),
        to: vec![recipient(None, "bob@example.test")],
        subject: "Status".to_string(),
        body: "Hello".to_string(),
        ..Default::default()
    };

    let message = build_smtp_message(&config, &request).expect("SMTP message");
    let formatted = String::from_utf8(message.formatted()).expect("message is UTF-8");

    assert!(formatted.contains("From:"));
    assert!(formatted.contains("Catch All"));
    assert!(formatted.contains("<catch@example.test>"));
}

#[test]
fn rejects_invalid_recipient_address() {
    let error = smtp_mailbox_for_recipient(&recipient(None, "not an address"))
        .expect_err("invalid address should be rejected");

    assert!(matches!(
        error,
        ImapAdapterError::InvalidSmtpAddress { address, .. } if address == "not an address"
    ));
}

#[test]
fn config_preserves_oauth2_auth_kind_for_xoauth2_sends() {
    let mut account = test_account(None, vec!["alice@example.test"], "alice-login");
    account.transport.provider = ProviderHint::Outlook;
    account.transport.auth = ProviderAuthKind::OAuth2;

    let config = SmtpConnectionConfig::from_account_settings(&account, "access-token".to_string())
        .expect("SMTP config");

    assert_eq!(config.auth, ProviderAuthKind::OAuth2);
    assert_eq!(config.provider, ProviderHint::Outlook);
    assert_eq!(config.secret, "access-token");
}

#[test]
fn config_from_account_settings_separates_auth_username_from_sender_email() {
    let account = test_account(
        Some("Alice Example"),
        vec!["*@example.test", "alice@example.test"],
        "alice-login",
    );

    let config = SmtpConnectionConfig::from_account_settings(&account, "secret".to_string())
        .expect("SMTP config");

    assert_eq!(config.username, "alice-login");
    assert_eq!(config.sender_email, "alice@example.test");
    assert_eq!(config.sender_name.as_deref(), Some("Alice Example"));
}

#[test]
fn config_from_account_settings_rejects_missing_sender_email() {
    let account = test_account(None, vec!["*@example.test"], "alice-login");

    let error = SmtpConnectionConfig::from_account_settings(&account, "secret".to_string())
        .expect_err("concrete sender email should be required");

    assert!(matches!(error, ImapAdapterError::MissingSmtpSenderEmail));
}

#[test]
fn config_from_account_settings_does_not_use_email_username_as_sender() {
    let account = test_account(None, Vec::new(), "alice@example.test");

    let error = SmtpConnectionConfig::from_account_settings(&account, "secret".to_string())
        .expect_err("configured sender email should be required");

    assert!(matches!(error, ImapAdapterError::MissingSmtpSenderEmail));
}

#[test]
fn provider_sent_copy_policy_avoids_known_auto_saved_providers() {
    assert_eq!(
        smtp_sent_copy_strategy(&ProviderHint::Gmail),
        SmtpSentCopyStrategy::ProviderManaged
    );
    assert_eq!(
        smtp_sent_copy_strategy(&ProviderHint::Outlook),
        SmtpSentCopyStrategy::ProviderManaged
    );
    assert_eq!(
        smtp_sent_copy_strategy(&ProviderHint::Generic),
        SmtpSentCopyStrategy::AppendToSentMailbox
    );
    assert_eq!(
        smtp_sent_copy_strategy(&ProviderHint::Icloud),
        SmtpSentCopyStrategy::AppendToSentMailbox
    );
}
