use std::collections::{BTreeMap, BTreeSet};

use posthaste_domain::{
    AccountId, ImapMailboxSyncState, ImapMessageLocation, ImapUid, ImapUidValidity, MailboxId,
    MailboxRecord, MessageId, MessageRecord, SyncBatch, SyncCursor, SyncObject,
};

use crate::{
    DiscoveredImapAccount, ImapChangedSinceSnapshot, ImapMailboxHeaderSnapshot, ImapMappedHeader,
};

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
        imap_mailbox_states: Vec::new(),
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

/// Convert IMAP discovery plus fetched mailbox headers into a full local
/// metadata snapshot.
///
/// The first IMAP sync path is intentionally full-snapshot based. UIDVALIDITY
/// and expunge handling make delta correctness mailbox-scoped; until that state
/// is wired through the runtime, the store's authoritative replacement contract
/// is the safer boundary.
///
/// @spec docs/L0-providers#imap-smtp-sync-strategy
/// @spec docs/L1-sync#syncbatch-and-apply_sync_batch
pub fn imap_full_sync_batch(
    account_id: &AccountId,
    discovery: DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
    mailbox_states: Vec<ImapMailboxSyncState>,
    updated_at: String,
) -> SyncBatch {
    let (messages, locations) = messages_and_locations_for_batch(&discovery, headers);
    let mut batch = imap_mailbox_sync_batch(account_id, discovery, updated_at.clone());

    batch.imap_mailbox_states = mailbox_states;
    batch.messages = messages;
    batch.imap_message_locations = locations;
    batch.replace_all_messages = true;
    batch.cursors.push(SyncCursor {
        object_type: SyncObject::Message,
        state: message_cursor_state(&batch.messages, &batch.imap_message_locations),
        updated_at,
    });
    batch
}

pub fn imap_delta_sync_batch(
    account_id: &AccountId,
    discovery: DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
    mailbox_states: Vec<ImapMailboxSyncState>,
    local_locations: Vec<ImapMessageLocation>,
    updated_at: String,
) -> SyncBatch {
    let remote_locations = headers
        .iter()
        .map(|header| {
            (
                header.location.mailbox_id.clone(),
                header.location.uid_validity.0,
                header.location.uid,
            )
        })
        .collect::<BTreeSet<_>>();
    let (messages, locations) = messages_and_locations_for_batch(&discovery, headers);
    let mut batch = imap_mailbox_sync_batch(account_id, discovery, updated_at.clone());
    let deleted_message_ids = local_locations
        .into_iter()
        .filter(|location| {
            !remote_locations.contains(&(
                location.mailbox_id.clone(),
                location.uid_validity.0,
                location.uid,
            ))
        })
        .map(|location| location.message_id)
        .collect::<Vec<_>>();

    batch.imap_mailbox_states = mailbox_states;
    batch.messages = messages;
    batch.imap_message_locations = locations;
    batch.deleted_message_ids = deleted_message_ids;
    batch.replace_all_messages = false;
    batch.cursors.push(SyncCursor {
        object_type: SyncObject::Message,
        state: message_cursor_state(&batch.messages, &batch.imap_message_locations),
        updated_at,
    });
    batch
}

pub fn imap_condstore_delta_sync_batch(
    account_id: &AccountId,
    discovery: DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
    mailbox_states: Vec<ImapMailboxSyncState>,
    local_locations: Vec<ImapMessageLocation>,
    vanished_uids: Vec<(MailboxId, ImapUidValidity, ImapUid)>,
    updated_at: String,
) -> SyncBatch {
    let (messages, locations) = messages_and_locations_for_batch(&discovery, headers);
    let mut batch = imap_mailbox_sync_batch(account_id, discovery, updated_at.clone());
    let vanished_locations = vanished_uids
        .into_iter()
        .map(|(mailbox_id, uid_validity, uid)| (mailbox_id, uid_validity.0, uid))
        .collect::<BTreeSet<_>>();
    let deleted_message_ids = local_locations
        .into_iter()
        .filter(|location| {
            vanished_locations.contains(&(
                location.mailbox_id.clone(),
                location.uid_validity.0,
                location.uid,
            ))
        })
        .map(|location| location.message_id)
        .collect::<Vec<_>>();

    batch.imap_mailbox_states = mailbox_states;
    batch.messages = messages;
    batch.imap_message_locations = locations;
    batch.deleted_message_ids = deleted_message_ids;
    batch.replace_all_messages = false;
    batch.cursors.push(SyncCursor {
        object_type: SyncObject::Message,
        state: message_cursor_state(&batch.messages, &batch.imap_message_locations),
        updated_at,
    });
    batch
}

pub fn imap_mailbox_state_from_header_snapshot(
    snapshot: &ImapMailboxHeaderSnapshot,
    updated_at: String,
) -> ImapMailboxSyncState {
    ImapMailboxSyncState {
        mailbox_id: snapshot.selected.mailbox_id.clone(),
        mailbox_name: snapshot.selected.mailbox_name.clone(),
        uid_validity: snapshot.selected.uid_validity,
        highest_uid: snapshot
            .headers
            .iter()
            .map(|header| header.location.uid)
            .max(),
        highest_modseq: snapshot
            .headers
            .iter()
            .filter_map(|header| header.location.modseq)
            .max(),
        updated_at,
    }
}

pub fn imap_mailbox_state_from_changed_since_snapshot(
    stored: &ImapMailboxSyncState,
    snapshot: &ImapChangedSinceSnapshot,
    updated_at: String,
) -> ImapMailboxSyncState {
    let mut state = ImapMailboxSyncState {
        mailbox_id: snapshot.selected.mailbox_id.clone(),
        mailbox_name: snapshot.selected.mailbox_name.clone(),
        uid_validity: snapshot.selected.uid_validity,
        highest_uid: stored.highest_uid,
        highest_modseq: stored.highest_modseq,
        updated_at,
    };

    for header in &snapshot.headers {
        state.record_seen_uid(header.location.uid);
        if let Some(modseq) = header.location.modseq {
            state.record_highest_modseq(modseq);
        }
    }
    if let Some(highest_modseq) = snapshot.selected.highest_modseq {
        state.record_highest_modseq(highest_modseq);
    }

    state
}

fn messages_and_locations_for_batch(
    discovery: &DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
) -> (Vec<MessageRecord>, Vec<ImapMessageLocation>) {
    let headers = canonicalize_imap_headers(discovery, headers);
    let mut messages_by_id = BTreeMap::<MessageId, MessageRecord>::new();
    let mut locations = Vec::with_capacity(headers.len());

    for header in headers {
        messages_by_id
            .entry(header.message.id.clone())
            .or_insert(header.message);
        locations.push(header.location);
    }

    (messages_by_id.into_values().collect(), locations)
}

fn canonicalize_imap_headers(
    discovery: &DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
) -> Vec<ImapMappedHeader> {
    if discovery.capabilities.supports_gmail_extensions() {
        canonicalize_gmail_headers(headers)
    } else {
        headers
    }
}

#[derive(Debug)]
struct GmailCanonicalMessageGroup {
    message: MessageRecord,
    mailbox_ids: BTreeSet<MailboxId>,
    keywords: BTreeSet<String>,
    locations: Vec<ImapMessageLocation>,
}

impl GmailCanonicalMessageGroup {
    fn new(message: MessageRecord) -> Self {
        Self {
            message,
            mailbox_ids: BTreeSet::new(),
            keywords: BTreeSet::new(),
            locations: Vec::new(),
        }
    }

    fn push(&mut self, mapped: ImapMappedHeader) {
        self.mailbox_ids.extend(mapped.message.mailbox_ids);
        self.keywords.extend(mapped.message.keywords);
        self.locations.push(mapped.location);
    }

    fn into_headers(mut self) -> Vec<ImapMappedHeader> {
        self.message.mailbox_ids = self.mailbox_ids.into_iter().collect();
        self.message.keywords = self.keywords.into_iter().collect();

        self.locations
            .into_iter()
            .map(|location| ImapMappedHeader {
                message: self.message.clone(),
                location,
            })
            .collect()
    }
}

fn canonicalize_gmail_headers(headers: Vec<ImapMappedHeader>) -> Vec<ImapMappedHeader> {
    let mut groups = BTreeMap::<MessageId, GmailCanonicalMessageGroup>::new();

    for mut header in headers {
        let canonical_id = gmail_canonical_message_id(&header.message);
        header.message.id = canonical_id.clone();
        header.location.message_id = canonical_id.clone();

        groups
            .entry(canonical_id)
            .or_insert_with(|| GmailCanonicalMessageGroup::new(header.message.clone()))
            .push(header);
    }

    groups
        .into_values()
        .flat_map(GmailCanonicalMessageGroup::into_headers)
        .collect()
}

fn gmail_canonical_message_id(message: &MessageRecord) -> MessageId {
    message
        .rfc_message_id
        .as_deref()
        .filter(|message_id| !message_id.is_empty())
        .map(|message_id| {
            MessageId(format!(
                "imap:gmail:rfc822msgid:{}",
                hex::encode(message_id.as_bytes())
            ))
        })
        .unwrap_or_else(|| message.id.clone())
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

fn message_cursor_state(messages: &[MessageRecord], locations: &[ImapMessageLocation]) -> String {
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

#[cfg(test)]
mod tests {
    use posthaste_domain::{
        ImapCapabilities, ImapMessageLocation, ImapModSeq, ImapSelectedMailbox, ImapUid,
        ImapUidValidity, MailboxId, MessageId,
    };

    use crate::{
        imap_header_message_record, map_imap_mailbox, ImapChangedSinceSnapshot, ImapFetchedHeader,
    };

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

    #[test]
    fn full_sync_batch_carries_messages_and_imap_locations() {
        let selected = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(9),
            uid_next: None,
            highest_modseq: None,
        };
        let mapped = imap_header_message_record(
            &selected,
            ImapFetchedHeader {
                mailbox_id: selected.mailbox_id.clone(),
                uid: ImapUid(42),
                modseq: Some(ImapModSeq(777)),
                flags: Vec::new(),
                rfc822_size: 512,
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("mapped header");
        let expected_location = ImapMessageLocation {
            message_id: mapped.message.id.clone(),
            mailbox_id: selected.mailbox_id.clone(),
            uid_validity: ImapUidValidity(9),
            uid: ImapUid(42),
            modseq: Some(ImapModSeq(777)),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };

        let batch = imap_full_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::default(),
                mailboxes: vec![map_imap_mailbox("INBOX", ["\\Inbox"])],
            },
            vec![mapped],
            vec![ImapMailboxSyncState {
                mailbox_id: selected.mailbox_id.clone(),
                mailbox_name: "INBOX".to_string(),
                uid_validity: ImapUidValidity(9),
                highest_uid: Some(ImapUid(42)),
                highest_modseq: Some(ImapModSeq(777)),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            }],
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert!(batch.replace_all_messages);
        assert_eq!(batch.messages.len(), 1);
        assert_eq!(batch.imap_mailbox_states.len(), 1);
        assert_eq!(batch.imap_message_locations, vec![expected_location]);
        assert_eq!(batch.cursors[1].object_type, SyncObject::Message);
        assert!(batch.cursors[1].state.starts_with("imap-messages:"));
    }

    #[test]
    fn gmail_sync_canonicalizes_one_message_observed_in_multiple_mailboxes() {
        let sent = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:sent"),
            mailbox_name: "[Gmail]/Sent Mail".to_string(),
            uid_validity: ImapUidValidity(7),
            uid_next: None,
            highest_modseq: None,
        };
        let starred = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:starred"),
            mailbox_name: "[Gmail]/Starred".to_string(),
            uid_validity: ImapUidValidity(8),
            uid_next: None,
            highest_modseq: None,
        };
        let sent_header = imap_header_message_record(
            &sent,
            ImapFetchedHeader {
                mailbox_id: sent.mailbox_id.clone(),
                uid: ImapUid(12),
                modseq: None,
                flags: vec!["\\Seen".to_string()],
                rfc822_size: 512,
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\nMessage-ID: <gmail-1@example.test>\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("sent header");
        let starred_header = imap_header_message_record(
            &starred,
            ImapFetchedHeader {
                mailbox_id: starred.mailbox_id.clone(),
                uid: ImapUid(44),
                modseq: None,
                flags: vec!["\\Flagged".to_string()],
                rfc822_size: 512,
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\nMessage-ID: <gmail-1@example.test>\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("starred header");

        let batch = imap_full_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::from_tokens(["IMAP4rev1", "X-GM-EXT-1"]),
                mailboxes: vec![
                    map_imap_mailbox("[Gmail]/Sent Mail", ["\\Sent"]),
                    map_imap_mailbox("[Gmail]/Starred", ["\\Flagged"]),
                ],
            },
            vec![sent_header, starred_header],
            Vec::new(),
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert_eq!(batch.messages.len(), 1);
        assert_eq!(
            batch.messages[0].id.as_str(),
            "imap:gmail:rfc822msgid:676d61696c2d31406578616d706c652e74657374"
        );
        assert_eq!(
            batch.messages[0].mailbox_ids,
            vec![
                MailboxId::from("imap:mailbox:sent"),
                MailboxId::from("imap:mailbox:starred")
            ]
        );
        assert_eq!(batch.messages[0].keywords, vec!["$flagged", "$seen"]);
        assert_eq!(batch.imap_message_locations.len(), 2);
        assert!(batch
            .imap_message_locations
            .iter()
            .all(|location| location.message_id == batch.messages[0].id));
    }

    #[test]
    fn generic_imap_keeps_mailbox_uid_identity_even_with_shared_rfc_message_id() {
        let inbox = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:inbox"),
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(7),
            uid_next: None,
            highest_modseq: None,
        };
        let archive = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:archive"),
            mailbox_name: "Archive".to_string(),
            uid_validity: ImapUidValidity(8),
            uid_next: None,
            highest_modseq: None,
        };
        let inbox_header = imap_header_message_record(
            &inbox,
            ImapFetchedHeader {
                mailbox_id: inbox.mailbox_id.clone(),
                uid: ImapUid(12),
                modseq: None,
                flags: Vec::new(),
                rfc822_size: 512,
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\nMessage-ID: <copied@example.test>\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("inbox header");
        let archive_header = imap_header_message_record(
            &archive,
            ImapFetchedHeader {
                mailbox_id: archive.mailbox_id.clone(),
                uid: ImapUid(44),
                modseq: None,
                flags: Vec::new(),
                rfc822_size: 512,
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\nMessage-ID: <copied@example.test>\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("archive header");

        let batch = imap_full_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::from_tokens(["IMAP4rev1"]),
                mailboxes: vec![
                    map_imap_mailbox("INBOX", ["\\Inbox"]),
                    map_imap_mailbox("Archive", ["\\Archive"]),
                ],
            },
            vec![inbox_header, archive_header],
            Vec::new(),
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert_eq!(batch.messages.len(), 2);
        assert_ne!(batch.messages[0].id, batch.messages[1].id);
        assert_eq!(batch.imap_message_locations.len(), 2);
    }

    #[test]
    fn delta_sync_batch_deletes_local_locations_missing_from_remote_mailbox() {
        let selected = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(9),
            uid_next: None,
            highest_modseq: None,
        };
        let mapped = imap_header_message_record(
            &selected,
            ImapFetchedHeader {
                mailbox_id: selected.mailbox_id.clone(),
                uid: ImapUid(42),
                modseq: Some(ImapModSeq(777)),
                flags: Vec::new(),
                rfc822_size: 512,
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("mapped header");
        let missing_location = ImapMessageLocation {
            message_id: MessageId::from("imap:9:41:696d61703a6d61696c626f783a34393465343234663538"),
            mailbox_id: selected.mailbox_id.clone(),
            uid_validity: selected.uid_validity,
            uid: ImapUid(41),
            modseq: Some(ImapModSeq(700)),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };

        let batch = imap_delta_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::default(),
                mailboxes: vec![map_imap_mailbox("INBOX", ["\\Inbox"])],
            },
            vec![mapped],
            vec![ImapMailboxSyncState {
                mailbox_id: selected.mailbox_id.clone(),
                mailbox_name: "INBOX".to_string(),
                uid_validity: selected.uid_validity,
                highest_uid: Some(ImapUid(42)),
                highest_modseq: Some(ImapModSeq(777)),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            }],
            vec![missing_location.clone()],
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert!(!batch.replace_all_messages);
        assert_eq!(batch.messages.len(), 1);
        assert_eq!(batch.deleted_message_ids, vec![missing_location.message_id]);
        assert_eq!(batch.cursors[1].object_type, SyncObject::Message);
    }

    #[test]
    fn condstore_delta_sync_batch_only_deletes_vanished_local_locations() {
        let selected = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(9),
            uid_next: None,
            highest_modseq: Some(ImapModSeq(900)),
        };
        let changed = imap_header_message_record(
            &selected,
            ImapFetchedHeader {
                mailbox_id: selected.mailbox_id.clone(),
                uid: ImapUid(42),
                modseq: Some(ImapModSeq(900)),
                flags: vec!["\\Seen".to_string()],
                rfc822_size: 512,
                has_attachment: false,
                headers: b"From: Alice <alice@example.test>\r\nSubject: Hello\r\n\r\n".to_vec(),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            },
        )
        .expect("mapped header");
        let unchanged_location = ImapMessageLocation {
            message_id: MessageId::from("imap:9:41:696d61703a6d61696c626f783a34393465343234663538"),
            mailbox_id: selected.mailbox_id.clone(),
            uid_validity: selected.uid_validity,
            uid: ImapUid(41),
            modseq: Some(ImapModSeq(700)),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };
        let vanished_location = ImapMessageLocation {
            message_id: MessageId::from("imap:9:40:696d61703a6d61696c626f783a34393465343234663538"),
            mailbox_id: selected.mailbox_id.clone(),
            uid_validity: selected.uid_validity,
            uid: ImapUid(40),
            modseq: Some(ImapModSeq(600)),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };

        let batch = imap_condstore_delta_sync_batch(
            &AccountId::from("primary"),
            DiscoveredImapAccount {
                capabilities: ImapCapabilities::default(),
                mailboxes: vec![map_imap_mailbox("INBOX", ["\\Inbox"])],
            },
            vec![changed],
            vec![ImapMailboxSyncState {
                mailbox_id: selected.mailbox_id.clone(),
                mailbox_name: "INBOX".to_string(),
                uid_validity: selected.uid_validity,
                highest_uid: Some(ImapUid(42)),
                highest_modseq: Some(ImapModSeq(900)),
                updated_at: "2026-04-25T00:00:00Z".to_string(),
            }],
            vec![unchanged_location.clone(), vanished_location.clone()],
            vec![(
                selected.mailbox_id.clone(),
                selected.uid_validity,
                vanished_location.uid,
            )],
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert!(!batch.replace_all_messages);
        assert_eq!(batch.messages.len(), 1);
        assert_eq!(
            batch.deleted_message_ids,
            vec![vanished_location.message_id]
        );
        assert!(!batch
            .deleted_message_ids
            .contains(&unchanged_location.message_id));
    }

    #[test]
    fn changed_since_snapshot_state_preserves_stored_uid_and_advances_modseq() {
        let selected = ImapSelectedMailbox {
            mailbox_id: MailboxId::from("imap:mailbox:494e424f58"),
            mailbox_name: "INBOX".to_string(),
            uid_validity: ImapUidValidity(9),
            uid_next: None,
            highest_modseq: Some(ImapModSeq(900)),
        };
        let stored = ImapMailboxSyncState {
            mailbox_id: selected.mailbox_id.clone(),
            mailbox_name: "INBOX".to_string(),
            uid_validity: selected.uid_validity,
            highest_uid: Some(ImapUid(100)),
            highest_modseq: Some(ImapModSeq(700)),
            updated_at: "2026-04-24T00:00:00Z".to_string(),
        };
        let snapshot = ImapChangedSinceSnapshot {
            selected: selected.clone(),
            headers: Vec::new(),
            vanished_uids: Vec::new(),
            is_full_snapshot: false,
        };

        let state = imap_mailbox_state_from_changed_since_snapshot(
            &stored,
            &snapshot,
            "2026-04-25T00:00:00Z".to_string(),
        );

        assert_eq!(state.highest_uid, Some(ImapUid(100)));
        assert_eq!(state.highest_modseq, Some(ImapModSeq(900)));
    }
}
