use jmap_client::mailbox;
use mail_domain::{BlobId, MailboxId, MailboxRecord, MessageId, MessageRecord, RFC3339_EPOCH};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

pub(crate) fn to_mailbox_record(mailbox: &jmap_client::mailbox::Mailbox) -> MailboxRecord {
    let role = match mailbox.role() {
        mailbox::Role::Inbox => Some("inbox".to_string()),
        mailbox::Role::Drafts => Some("drafts".to_string()),
        mailbox::Role::Sent => Some("sent".to_string()),
        mailbox::Role::Trash => Some("trash".to_string()),
        mailbox::Role::Junk => Some("junk".to_string()),
        mailbox::Role::Archive => Some("archive".to_string()),
        mailbox::Role::None => None,
        other => Some(format!("{other:?}").to_lowercase()),
    };
    MailboxRecord {
        id: MailboxId(mailbox.id().unwrap_or_default().to_string()),
        name: mailbox.name().unwrap_or("(unnamed)").to_string(),
        role,
        unread_emails: mailbox.unread_emails() as i64,
        total_emails: mailbox.total_emails() as i64,
    }
}

pub(crate) fn to_message_record(email: &jmap_client::email::Email) -> MessageRecord {
    let (from_name, from_email) = email
        .from()
        .and_then(|addresses| addresses.first())
        .map(|address| {
            (
                address.name().map(String::from),
                Some(address.email().to_string()),
            )
        })
        .unwrap_or((None, None));
    MessageRecord {
        id: MessageId(email.id().unwrap_or_default().to_string()),
        source_thread_id: mail_domain::ThreadId(
            email.thread_id().unwrap_or_default().to_string(),
        ),
        remote_blob_id: email.blob_id().map(|blob_id| BlobId(blob_id.to_string())),
        subject: email.subject().map(String::from),
        from_name,
        from_email,
        preview: email.preview().map(String::from),
        received_at: email
            .received_at()
            .and_then(timestamp_to_iso8601)
            .unwrap_or_else(|| RFC3339_EPOCH.to_string()),
        has_attachment: email.has_attachment(),
        size: email.size() as i64,
        mailbox_ids: email
            .mailbox_ids()
            .into_iter()
            .map(|id| MailboxId(id.to_string()))
            .collect(),
        keywords: email.keywords().into_iter().map(String::from).collect(),
        body_html: None,
        body_text: None,
        raw_mime: None,
        rfc_message_id: email.message_id().and_then(|ids| ids.first()).cloned(),
        in_reply_to: email.in_reply_to().and_then(|ids| ids.first()).cloned(),
        references: email
            .references()
            .map(|references| references.to_vec())
            .unwrap_or_default(),
    }
}

fn timestamp_to_iso8601(timestamp: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp(timestamp)
        .ok()
        .and_then(|value| value.format(&Rfc3339).ok())
}
