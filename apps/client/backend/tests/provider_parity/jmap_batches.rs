use posthaste_domain_model::{
    MailboxId, MailboxRecord, MessageId, MessageRecord, SyncBatch, SyncCursor, SyncObject, ThreadId,
};

pub(super) fn jmap_sync_batch() -> SyncBatch {
    SyncBatch {
        mailboxes: vec![MailboxRecord {
            id: MailboxId::from("inbox"),
            name: "Inbox".to_string(),
            role: Some("inbox".to_string()),
            unread_emails: 0,
            total_emails: 1,
        }],
        messages: vec![MessageRecord {
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: Vec::new(),
            id: MessageId::from("jmap-message-1"),
            source_thread_id: ThreadId::from("thread-1"),
            remote_blob_id: None,
            subject: Some("Parity subject".to_string()),
            from_name: Some("Alice".to_string()),
            from_email: Some("alice@example.test".to_string()),
            to: Vec::new(),
            preview: None,
            received_at: "2026-04-25T12:00:00Z".to_string(),
            has_attachment: true,
            size: 512,
            mailbox_ids: vec![MailboxId::from("inbox")],
            keywords: vec!["$flagged".to_string(), "$seen".to_string()],
            body_html: None,
            body_text: None,
            raw_mime: None,
            rfc_message_id: Some("<parity@example.test>".to_string()),
            in_reply_to: None,
            references: Vec::new(),
            draft_id: None,
            list_unsubscribe: None,
        }],
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_imap_message_locations: Vec::new(),
        deleted_mailbox_ids: Vec::new(),
        absence_deleted_imap_message_locations: Vec::new(),
        absence_deleted_message_ids: Vec::new(),
        deleted_message_ids: Vec::new(),
        replace_all_mailboxes: true,
        replace_all_messages: true,
        cursors: vec![
            SyncCursor {
                object_type: SyncObject::Mailbox,
                state: "jmap-mailbox-state".to_string(),
                updated_at: "2026-04-25T12:00:00Z".to_string(),
            },
            SyncCursor {
                object_type: SyncObject::Message,
                state: "jmap-message-state".to_string(),
                updated_at: "2026-04-25T12:00:00Z".to_string(),
            },
        ],
    }
}

pub(super) fn jmap_label_initial_batch() -> SyncBatch {
    jmap_label_batch(
        vec![
            MailboxId::from("jmap-inbox"),
            MailboxId::from("jmap-archive"),
        ],
        "jmap-label-state-1",
    )
}

pub(super) fn jmap_label_removed_batch() -> SyncBatch {
    jmap_label_batch(vec![MailboxId::from("jmap-archive")], "jmap-label-state-2")
}

fn jmap_label_batch(mailbox_ids: Vec<MailboxId>, state: &str) -> SyncBatch {
    SyncBatch {
        mailboxes: vec![
            MailboxRecord {
                id: MailboxId::from("jmap-inbox"),
                name: "Inbox".to_string(),
                role: Some("inbox".to_string()),
                unread_emails: 0,
                total_emails: 1,
            },
            MailboxRecord {
                id: MailboxId::from("jmap-archive"),
                name: "Archive".to_string(),
                role: Some("archive".to_string()),
                unread_emails: 0,
                total_emails: 1,
            },
        ],
        messages: vec![MessageRecord {
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: Vec::new(),
            id: MessageId::from("jmap-label-message"),
            source_thread_id: ThreadId::from("jmap-label-thread"),
            remote_blob_id: None,
            subject: Some("Label parity".to_string()),
            from_name: Some("Alice".to_string()),
            from_email: Some("alice@example.test".to_string()),
            to: Vec::new(),
            preview: None,
            received_at: "2026-04-25T12:00:00Z".to_string(),
            has_attachment: false,
            size: 256,
            mailbox_ids,
            keywords: vec!["$seen".to_string()],
            body_html: None,
            body_text: None,
            raw_mime: None,
            rfc_message_id: Some("<label-parity@example.test>".to_string()),
            in_reply_to: None,
            references: Vec::new(),
            draft_id: None,
            list_unsubscribe: None,
        }],
        imap_mailbox_states: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_imap_message_locations: Vec::new(),
        deleted_mailbox_ids: Vec::new(),
        absence_deleted_imap_message_locations: Vec::new(),
        absence_deleted_message_ids: Vec::new(),
        deleted_message_ids: Vec::new(),
        replace_all_mailboxes: true,
        replace_all_messages: false,
        cursors: vec![SyncCursor {
            object_type: SyncObject::Message,
            state: state.to_string(),
            updated_at: "2026-04-25T12:00:00Z".to_string(),
        }],
    }
}
