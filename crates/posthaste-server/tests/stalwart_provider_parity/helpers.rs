use std::collections::BTreeSet;

use posthaste_domain::{
    AccountId, ImapMessageLocationStore, MailboxId, MailboxSummary, MessageSummary, SyncTrigger,
};
use posthaste_engine::LiveJmapGateway;
use posthaste_imap::LiveImapSmtpGateway;

use posthaste_testkit::Harness;

pub(super) async fn sync_pair(
    harness: &Harness,
    jmap_gateway: &LiveJmapGateway,
    imap_gateway: &LiveImapSmtpGateway,
) {
    harness
        .service
        .sync_account(
            &AccountId::from("jmap-stalwart"),
            SyncTrigger::Manual,
            jmap_gateway,
            None,
        )
        .await
        .expect("JMAP sync should succeed");
    harness
        .service
        .sync_account(
            &AccountId::from("imap-stalwart"),
            SyncTrigger::Manual,
            imap_gateway,
            None,
        )
        .await
        .expect("IMAP sync should succeed");
}

pub(super) fn normalized_messages(harness: &Harness, account_id: &str) -> Vec<String> {
    let mut messages = harness
        .service
        .list_messages(&AccountId::from(account_id), None)
        .expect("messages should list")
        .into_iter()
        .map(|message| {
            format!(
                "{}\0{}\0{}\0{}\0{}",
                message.subject.unwrap_or_default(),
                message.from_email.unwrap_or_default(),
                message.has_attachment,
                message.is_read,
                message.is_flagged
            )
        })
        .collect::<Vec<_>>();
    messages.sort();
    messages
}

pub(super) fn jmap_message_by_subject(
    harness: &Harness,
    account_id: &str,
    subject: &str,
) -> MessageSummary {
    message_by_subject(harness, account_id, subject)
}

pub(super) fn message_by_subject(
    harness: &Harness,
    account_id: &str,
    subject: &str,
) -> MessageSummary {
    maybe_message_by_subject(harness, account_id, subject)
        .unwrap_or_else(|| panic!("message with subject {subject:?} should exist"))
}

pub(super) fn maybe_message_by_subject(
    harness: &Harness,
    account_id: &str,
    subject: &str,
) -> Option<MessageSummary> {
    harness
        .service
        .list_messages(&AccountId::from(account_id), None)
        .expect("messages should list")
        .into_iter()
        .find(|message| message.subject.as_deref() == Some(subject))
}

pub(super) fn mailbox_id_by_label(harness: &Harness, account_id: &str, label: &str) -> MailboxId {
    harness
        .service
        .list_mailboxes(&AccountId::from(account_id))
        .expect("mailboxes should list")
        .into_iter()
        .find(|mailbox| mailbox_label(mailbox) == label)
        .map(|mailbox| mailbox.id)
        .unwrap_or_else(|| panic!("mailbox with label {label:?} should exist"))
}

pub(super) fn mailbox_labels_for_subject(
    harness: &Harness,
    account_id: &str,
    subject: &str,
) -> BTreeSet<String> {
    let mailboxes = harness
        .service
        .list_mailboxes(&AccountId::from(account_id))
        .expect("mailboxes should list")
        .into_iter()
        .map(|mailbox| (mailbox.id.clone(), mailbox_label(&mailbox)))
        .collect::<std::collections::BTreeMap<_, _>>();
    message_by_subject(harness, account_id, subject)
        .mailbox_ids
        .into_iter()
        .map(|mailbox_id| {
            mailboxes
                .get(&mailbox_id)
                .cloned()
                .unwrap_or_else(|| mailbox_id.to_string())
        })
        .collect()
}

fn mailbox_label(mailbox: &MailboxSummary) -> String {
    mailbox
        .role
        .clone()
        .unwrap_or_else(|| mailbox.name.to_ascii_lowercase())
}

pub(super) fn imap_location_count(harness: &Harness) -> usize {
    harness
        .service
        .list_messages(&AccountId::from("imap-stalwart"), None)
        .expect("IMAP messages should list")
        .into_iter()
        .map(|message| {
            harness
                .store
                .list_imap_message_locations(&AccountId::from("imap-stalwart"), &message.id)
                .expect("IMAP locations should list")
                .len()
        })
        .sum()
}
