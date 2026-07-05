use posthaste_domain_model::{AccountId, ProviderKind, ProviderProfile, SyncBatch};
use posthaste_imap::{
    imap_condstore_delta_sync_batch, imap_full_sync_batch, imap_header_message_record,
    imap_mailbox_state_from_header_snapshot, map_imap_mailbox, map_imap_mailbox_with_provider,
    ImapFetchedHeader, ImapMailboxHeaderSnapshot,
};

pub(super) fn imap_sync_batch() -> SyncBatch {
    let selected = posthaste_domain_model::ImapSelectedMailbox {
        mailbox_id: posthaste_imap::imap_mailbox_id("INBOX"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: posthaste_domain_model::ImapUidValidity(9),
        uid_next: None,
        highest_modseq: None,
    };
    let mapped = imap_header_message_record(
        &selected,
        ImapFetchedHeader {
            mailbox_id: selected.mailbox_id.clone(),
            uid: posthaste_domain_model::ImapUid(42),
            modseq: Some(posthaste_domain_model::ImapModSeq(777)),
            flags: vec!["\\Seen".to_string(), "\\Flagged".to_string()],
            rfc822_size: 512,
            has_attachment: true,
            headers: concat!(
                "From: Alice <alice@example.test>\r\n",
                "Date: Sat, 25 Apr 2026 12:00:00 +0000\r\n",
                "Subject: Parity subject\r\n",
                "Message-ID: <parity@example.test>\r\n",
                "\r\n",
            )
            .as_bytes()
            .to_vec(),
            updated_at: "2026-04-25T12:00:00Z".to_string(),
        },
    )
    .expect("IMAP header should map");
    let snapshot = ImapMailboxHeaderSnapshot {
        selected,
        headers: vec![mapped.clone()],
    };

    imap_full_sync_batch(
        &AccountId::from("imap"),
        posthaste_imap::DiscoveredImapAccount {
            capabilities: posthaste_domain_model::ImapCapabilities::default(),
            mailboxes: vec![map_imap_mailbox("INBOX", ["\\Inbox"])],
        },
        vec![mapped],
        vec![imap_mailbox_state_from_header_snapshot(
            &snapshot,
            "2026-04-25T12:00:00Z".to_string(),
        )],
        "2026-04-25T12:00:00Z".to_string(),
    )
}

pub(super) fn imap_gmail_label_sync_batch() -> SyncBatch {
    let inbox = posthaste_domain_model::ImapSelectedMailbox {
        mailbox_id: posthaste_imap::imap_mailbox_id("INBOX"),
        mailbox_name: "INBOX".to_string(),
        uid_validity: posthaste_domain_model::ImapUidValidity(11),
        uid_next: None,
        highest_modseq: Some(posthaste_domain_model::ImapModSeq(900)),
    };
    let archive = posthaste_domain_model::ImapSelectedMailbox {
        mailbox_id: posthaste_imap::imap_mailbox_id("[Gmail]/All Mail"),
        mailbox_name: "[Gmail]/All Mail".to_string(),
        uid_validity: posthaste_domain_model::ImapUidValidity(12),
        uid_next: None,
        highest_modseq: Some(posthaste_domain_model::ImapModSeq(901)),
    };
    let inbox_header = imap_label_header(&inbox, posthaste_domain_model::ImapUid(101), &["\\Seen"]);
    let archive_header = imap_label_header(&archive, posthaste_domain_model::ImapUid(202), &["\\Seen"]);
    let inbox_snapshot = ImapMailboxHeaderSnapshot {
        selected: inbox,
        headers: vec![inbox_header.clone()],
    };
    let archive_snapshot = ImapMailboxHeaderSnapshot {
        selected: archive,
        headers: vec![archive_header.clone()],
    };

    imap_full_sync_batch(
        &AccountId::from("imap-labels"),
        posthaste_imap::DiscoveredImapAccount {
            capabilities: posthaste_domain_model::ImapCapabilities::from_tokens([
                "IMAP4rev1",
                "CONDSTORE",
                "QRESYNC",
                "X-GM-EXT-1",
            ]),
            mailboxes: vec![
                map_imap_mailbox("INBOX", ["\\Inbox"]),
                map_gmail_imap_mailbox("[Gmail]/All Mail", ["\\All"]),
            ],
        },
        vec![inbox_header, archive_header],
        vec![
            imap_mailbox_state_from_header_snapshot(
                &inbox_snapshot,
                "2026-04-25T12:00:00Z".to_string(),
            ),
            imap_mailbox_state_from_header_snapshot(
                &archive_snapshot,
                "2026-04-25T12:00:00Z".to_string(),
            ),
        ],
        "2026-04-25T12:00:00Z".to_string(),
    )
}

pub(super) fn imap_gmail_flagged_delta_batch(
    locations: Vec<posthaste_domain_model::ImapMessageLocation>,
) -> SyncBatch {
    let archive = posthaste_domain_model::ImapSelectedMailbox {
        mailbox_id: posthaste_imap::imap_mailbox_id("[Gmail]/All Mail"),
        mailbox_name: "[Gmail]/All Mail".to_string(),
        uid_validity: posthaste_domain_model::ImapUidValidity(12),
        uid_next: None,
        highest_modseq: Some(posthaste_domain_model::ImapModSeq(1102)),
    };
    let changed_header = imap_label_header(
        &archive,
        posthaste_domain_model::ImapUid(202),
        &["\\Seen", "\\Flagged"],
    );

    imap_condstore_delta_sync_batch(
        &AccountId::from("imap-flags"),
        posthaste_imap::DiscoveredImapAccount {
            capabilities: posthaste_domain_model::ImapCapabilities::from_tokens([
                "IMAP4rev1",
                "CONDSTORE",
                "QRESYNC",
                "X-GM-EXT-1",
            ]),
            mailboxes: vec![
                map_imap_mailbox("INBOX", ["\\Inbox"]),
                map_gmail_imap_mailbox("[Gmail]/All Mail", ["\\All"]),
            ],
        },
        vec![changed_header],
        vec![posthaste_domain_model::ImapMailboxSyncState {
            mailbox_id: archive.mailbox_id.clone(),
            mailbox_name: archive.mailbox_name,
            uid_validity: archive.uid_validity,
            highest_uid: Some(posthaste_domain_model::ImapUid(202)),
            highest_modseq: archive.highest_modseq,
            partial_initial_uid: None,
            updated_at: "2026-04-25T12:01:00Z".to_string(),
        }],
        locations,
        Vec::new(),
        Vec::new(),
        "2026-04-25T12:01:00Z".to_string(),
    )
}

pub(super) fn imap_single_label_vanished_batch(
    locations: Vec<posthaste_domain_model::ImapMessageLocation>,
) -> SyncBatch {
    let vanished = locations
        .iter()
        .find(|location| location.mailbox_id == posthaste_imap::imap_mailbox_id("INBOX"))
        .map(|location| {
            (
                location.mailbox_id.clone(),
                location.uid_validity,
                location.uid,
            )
        })
        .expect("initial fixture should have an INBOX location");

    imap_condstore_delta_sync_batch(
        &AccountId::from("imap-labels"),
        posthaste_imap::DiscoveredImapAccount {
            capabilities: posthaste_domain_model::ImapCapabilities::from_tokens([
                "IMAP4rev1",
                "CONDSTORE",
                "QRESYNC",
                "X-GM-EXT-1",
            ]),
            mailboxes: vec![
                map_imap_mailbox("INBOX", ["\\Inbox"]),
                map_gmail_imap_mailbox("[Gmail]/All Mail", ["\\All"]),
            ],
        },
        Vec::new(),
        Vec::new(),
        locations,
        vec![vanished],
        Vec::new(),
        "2026-04-25T12:01:00Z".to_string(),
    )
}

fn map_gmail_imap_mailbox(
    name: impl Into<String>,
    attributes: impl IntoIterator<Item = impl AsRef<str>>,
) -> posthaste_imap::DiscoveredImapMailbox {
    map_imap_mailbox_with_provider(
        ProviderProfile::from_kind(ProviderKind::Gmail),
        name,
        attributes,
    )
}

fn imap_label_header(
    selected: &posthaste_domain_model::ImapSelectedMailbox,
    uid: posthaste_domain_model::ImapUid,
    flags: &[&str],
) -> posthaste_imap::ImapMappedHeader {
    imap_header_message_record(
        selected,
        ImapFetchedHeader {
            mailbox_id: selected.mailbox_id.clone(),
            uid,
            modseq: Some(posthaste_domain_model::ImapModSeq(900 + uid.0 as u64)),
            flags: flags.iter().map(|flag| (*flag).to_string()).collect(),
            rfc822_size: 256,
            has_attachment: false,
            headers: concat!(
                "From: Alice <alice@example.test>\r\n",
                "Date: Sat, 25 Apr 2026 12:00:00 +0000\r\n",
                "Subject: Label parity\r\n",
                "Message-ID: <label-parity@example.test>\r\n",
                "\r\n",
            )
            .as_bytes()
            .to_vec(),
            updated_at: "2026-04-25T12:00:00Z".to_string(),
        },
    )
    .expect("IMAP label header should map")
}
