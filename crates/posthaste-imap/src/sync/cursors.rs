use posthaste_domain_service::{ImapMessageLocation, MailboxRecord, MessageRecord};

pub(super) fn mailbox_cursor_state(mailboxes: &[MailboxRecord]) -> String {
    let mut fingerprint = String::new();
    for mailbox in mailboxes {
        fingerprint.push_str(mailbox.id.as_str());
        fingerprint.push('\0');
        fingerprint.push_str(&mailbox.name);
        fingerprint.push('\0');
        fingerprint.push_str(mailbox.role.as_deref().unwrap_or(""));
        fingerprint.push('\0');
    }
    format!("imap-mailboxes:{}", hex::encode(fingerprint.as_bytes()))
}

pub(super) fn message_cursor_state(
    messages: &[MessageRecord],
    locations: &[ImapMessageLocation],
) -> String {
    let mut fingerprint = String::new();
    for message in messages {
        fingerprint.push_str(message.id.as_str());
        fingerprint.push('\0');
    }
    for location in locations {
        fingerprint.push_str(location.message_id.as_str());
        fingerprint.push('\0');
        fingerprint.push_str(location.mailbox_id.as_str());
        fingerprint.push('\0');
        fingerprint.push_str(&location.uid_validity.0.to_string());
        fingerprint.push('\0');
        fingerprint.push_str(&location.uid.0.to_string());
        fingerprint.push('\0');
    }
    format!("imap-messages:{}", hex::encode(fingerprint.as_bytes()))
}
