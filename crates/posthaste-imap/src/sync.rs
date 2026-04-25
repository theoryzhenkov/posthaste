use posthaste_domain::{AccountId, MailboxRecord, SyncBatch, SyncCursor, SyncObject};

use crate::DiscoveredImapAccount;

/// Convert an IMAP mailbox discovery result into an authoritative mailbox
/// snapshot. Message sync is intentionally separate because it depends on
/// per-mailbox UIDVALIDITY and UID fetch state.
///
/// @spec docs/L0-providers#imap-discovery-runtime
pub fn imap_mailbox_sync_batch(
    _account_id: &AccountId,
    discovery: DiscoveredImapAccount,
    updated_at: String,
) -> SyncBatch {
    let mailboxes = discovery
        .mailboxes
        .iter()
        .filter(|mailbox| mailbox.selectable)
        .map(|mailbox| MailboxRecord {
            id: mailbox.id.clone(),
            name: mailbox.name.clone(),
            role: mailbox.role.map(str::to_string),
            unread_emails: 0,
            total_emails: 0,
        })
        .collect::<Vec<_>>();
    let cursor_state = mailbox_cursor_state(&mailboxes);

    SyncBatch {
        mailboxes,
        messages: Vec::new(),
        imap_message_locations: Vec::new(),
        deleted_mailbox_ids: Vec::new(),
        deleted_message_ids: Vec::new(),
        replace_all_mailboxes: true,
        replace_all_messages: false,
        cursors: vec![SyncCursor {
            object_type: SyncObject::Mailbox,
            state: cursor_state,
            updated_at,
        }],
    }
}

fn mailbox_cursor_state(mailboxes: &[MailboxRecord]) -> String {
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

#[cfg(test)]
mod tests {
    use posthaste_domain::{ImapCapabilities, MailboxId};

    use crate::{map_imap_mailbox, DiscoveredImapAccount};

    use super::*;

    #[test]
    fn mailbox_discovery_becomes_authoritative_mailbox_snapshot() {
        let batch = imap_mailbox_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::default(),
                mailboxes: vec![
                    map_imap_mailbox("INBOX", ["\\Inbox"]),
                    map_imap_mailbox("[Gmail]", ["\\Noselect"]),
                    map_imap_mailbox("[Gmail]/Sent Mail", ["\\Sent"]),
                ],
            },
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert!(batch.replace_all_mailboxes);
        assert!(!batch.replace_all_messages);
        assert_eq!(batch.mailboxes.len(), 2);
        assert_eq!(
            batch.mailboxes[0].id,
            MailboxId::from("imap:mailbox:494e424f58")
        );
        assert_eq!(batch.mailboxes[0].role.as_deref(), Some("inbox"));
        assert_eq!(batch.mailboxes[1].role.as_deref(), Some("sent"));
        assert_eq!(batch.cursors[0].object_type, SyncObject::Mailbox);
        assert!(batch.cursors[0].state.starts_with("imap-mailboxes:"));
    }
}
