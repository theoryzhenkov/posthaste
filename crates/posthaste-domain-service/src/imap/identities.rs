use super::*;

/// Build a stable local message ID for an IMAP message.
///
/// The mailbox identity and UIDVALIDITY are part of the ID so UID reuse after a
/// server-side mailbox reset cannot alias a previously cached message.
///
/// @spec docs/L0-providers#identity-and-threading
pub fn imap_message_id(
    mailbox_id: &MailboxId,
    uid_validity: ImapUidValidity,
    uid: ImapUid,
) -> MessageId {
    MessageId(format!(
        "imap:{}:{}:{}",
        uid_validity.0,
        uid.0,
        hex_encode(mailbox_id.as_str().as_bytes())
    ))
}

/// Build a stable local message ID from Gmail's `X-GM-MSGID`.
///
/// Gmail exposes the same message through multiple labels/mailboxes, so UID is
/// not the best deduplication key when the extension is available.
///
/// @spec docs/L0-providers#identity-and-threading
pub fn gmail_message_id(gmail_id: GmailMessageId) -> MessageId {
    MessageId(format!("imap:gmail:msgid:{}", gmail_id.0))
}

/// Build a stable local thread ID from Gmail's `X-GM-THRID`.
///
/// @spec docs/L0-providers#identity-and-threading
pub fn gmail_thread_id(gmail_id: GmailThreadId) -> posthaste_domain_model::ThreadId {
    posthaste_domain_model::ThreadId(format!("imap:gmail:thrid:{}", gmail_id.0))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}
