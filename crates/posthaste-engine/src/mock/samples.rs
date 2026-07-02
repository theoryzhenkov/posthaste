use posthaste_domain_model::{
    BlobId, MailboxId, MailboxRecord, MessageAttachment, MessageId, MessageRecord,
};

pub(super) fn sample_mailboxes() -> Vec<MailboxRecord> {
    vec![
        MailboxRecord {
            id: MailboxId::from("mb-inbox"),
            name: "Inbox".to_string(),
            role: Some("inbox".to_string()),
            unread_emails: 2,
            total_emails: 3,
        },
        MailboxRecord {
            id: MailboxId::from("mb-archive"),
            name: "Archive".to_string(),
            role: Some("archive".to_string()),
            unread_emails: 0,
            total_emails: 0,
        },
        MailboxRecord {
            id: MailboxId::from("mb-trash"),
            name: "Trash".to_string(),
            role: Some("trash".to_string()),
            unread_emails: 0,
            total_emails: 0,
        },
    ]
}

/// Seed data: three messages across two threads with pre-populated bodies.
pub(super) fn sample_messages() -> Vec<MessageRecord> {
    vec![
        MessageRecord {
            id: MessageId::from("em-001"),
            source_thread_id: posthaste_domain_model::ThreadId::from("th-roadmap"),
            remote_blob_id: None,
            subject: Some("Q2 planning priorities".to_string()),
            from_name: Some("Alice Chen".to_string()),
            from_email: Some("alice@example.com".to_string()),
            to: Vec::new(),
            preview: Some("Roadmap draft attached.".to_string()),
            received_at: "2026-03-31T09:00:00Z".to_string(),
            has_attachment: true,
            size: 48120,
            mailbox_ids: vec![MailboxId::from("mb-inbox")],
            keywords: vec!["$seen".to_string(), "$flagged".to_string()],
            body_html: Some("<p>Roadmap draft attached.</p>".to_string()),
            body_text: Some("Roadmap draft attached.".to_string()),
            raw_mime: Some("From: Alice <alice@example.com>\r\nSubject: Q2 planning priorities\r\n\r\nRoadmap draft attached.\r\n".to_string()),
            rfc_message_id: Some("<em-001@mock>".to_string()),
            in_reply_to: None,
            references: Vec::new(),
            draft_id: None,
        },
        MessageRecord {
            id: MessageId::from("em-002"),
            source_thread_id: posthaste_domain_model::ThreadId::from("th-roadmap"),
            remote_blob_id: None,
            subject: Some("Re: Q2 planning priorities".to_string()),
            from_name: Some("Marcus Johnson".to_string()),
            from_email: Some("marcus@example.com".to_string()),
            to: Vec::new(),
            preview: Some("Looks good; one question on staffing.".to_string()),
            received_at: "2026-03-31T09:30:00Z".to_string(),
            has_attachment: false,
            size: 4120,
            mailbox_ids: vec![MailboxId::from("mb-inbox"), MailboxId::from("mb-archive")],
            keywords: Vec::new(),
            body_html: Some("<p>Looks good; one question on staffing.</p>".to_string()),
            body_text: Some("Looks good; one question on staffing.".to_string()),
            raw_mime: Some("From: Marcus <marcus@example.com>\r\nSubject: Re: Q2 planning priorities\r\n\r\nLooks good; one question on staffing.\r\n".to_string()),
            rfc_message_id: Some("<em-002@mock>".to_string()),
            in_reply_to: Some("<em-001@mock>".to_string()),
            references: vec!["<em-001@mock>".to_string()],
            draft_id: None,
        },
        MessageRecord {
            id: MessageId::from("em-003"),
            source_thread_id: posthaste_domain_model::ThreadId::from("th-invoice"),
            remote_blob_id: None,
            subject: Some("Invoice #2026-0312".to_string()),
            from_name: Some("Cloudflare Billing".to_string()),
            from_email: Some("billing@cloudflare.com".to_string()),
            to: Vec::new(),
            preview: Some("Your March invoice is ready.".to_string()),
            received_at: "2026-03-30T15:00:00Z".to_string(),
            has_attachment: true,
            size: 52010,
            mailbox_ids: vec![MailboxId::from("mb-inbox")],
            keywords: vec!["$seen".to_string()],
            body_html: Some("<p>Your March invoice is ready.</p>".to_string()),
            body_text: Some("Your March invoice is ready.".to_string()),
            raw_mime: Some("From: Cloudflare Billing <billing@cloudflare.com>\r\nSubject: Invoice #2026-0312\r\n\r\nYour March invoice is ready.\r\n".to_string()),
            rfc_message_id: Some("<em-003@mock>".to_string()),
            in_reply_to: None,
            references: Vec::new(),
            draft_id: None,
        },
    ]
}

pub(super) fn sample_attachments(message_id: &str) -> Vec<MessageAttachment> {
    match message_id {
        "em-001" => vec![MessageAttachment {
            id: "attachment-1".to_string(),
            blob_id: BlobId::from("blob-roadmap".to_string()),
            part_id: Some("2".to_string()),
            filename: Some("roadmap.md".to_string()),
            mime_type: "text/markdown".to_string(),
            size: 42,
            disposition: Some("attachment".to_string()),
            cid: None,
            is_inline: false,
        }],
        "em-003" => vec![MessageAttachment {
            id: "attachment-1".to_string(),
            blob_id: BlobId::from("blob-invoice".to_string()),
            part_id: Some("2".to_string()),
            filename: Some("invoice-2026-0312.txt".to_string()),
            mime_type: "text/plain".to_string(),
            size: 58,
            disposition: Some("attachment".to_string()),
            cid: None,
            is_inline: false,
        }],
        _ => Vec::new(),
    }
}

pub(super) fn sample_attachment_bytes(blob_id: &str) -> Option<Vec<u8>> {
    match blob_id {
        "blob-roadmap" => {
            Some(b"# Q2 roadmap\n\n- Search\n- Attachments\n- Compose polish\n".to_vec())
        }
        "blob-invoice" => Some(b"Invoice 2026-0312\nAmount due: $42.00\nStatus: Paid\n".to_vec()),
        _ => None,
    }
}
