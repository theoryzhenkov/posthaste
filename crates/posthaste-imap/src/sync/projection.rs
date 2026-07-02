use std::collections::{BTreeMap, BTreeSet};

use posthaste_domain_service::{ImapMessageLocation, MailboxId, MessageId, MessageRecord};

use crate::{
    message::ImapMailboxMembershipSource, provider::ImapAdapterProviderProfile,
    DiscoveredImapAccount, ImapMappedHeader,
};

pub(super) fn messages_and_locations_for_batch(
    discovery: &DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
) -> ProjectedMessages {
    messages_and_locations_from_headers(project_imap_headers(discovery, headers))
}

pub(super) struct ProjectedMessages {
    pub(super) messages: Vec<MessageRecord>,
    pub(super) locations: Vec<ImapMessageLocation>,
    pub(super) provider_absent_mailbox_ids_by_message: BTreeMap<MessageId, BTreeSet<MailboxId>>,
}

pub(super) fn messages_and_locations_from_headers(
    headers: Vec<ImapMappedHeader>,
) -> ProjectedMessages {
    let mut messages_by_id = BTreeMap::<MessageId, MessageRecord>::new();
    let mut locations = Vec::with_capacity(headers.len());
    let mut provider_absent_mailbox_ids_by_message =
        BTreeMap::<MessageId, BTreeSet<MailboxId>>::new();

    for header in headers {
        if header.mailbox_membership_source == ImapMailboxMembershipSource::ProviderLabels {
            provider_absent_mailbox_ids_by_message
                .entry(header.message.id.clone())
                .or_default()
                .extend(header.provider_absent_mailbox_ids.iter().cloned());
        }
        messages_by_id
            .entry(header.message.id.clone())
            .or_insert(header.message);
        locations.push(header.location);
    }

    ProjectedMessages {
        messages: messages_by_id.into_values().collect(),
        locations,
        provider_absent_mailbox_ids_by_message,
    }
}

pub(super) fn project_imap_headers(
    discovery: &DiscoveredImapAccount,
    headers: Vec<ImapMappedHeader>,
) -> Vec<ImapMappedHeader> {
    ImapAdapterProviderProfile::from_discovery(discovery).project_headers(headers)
}
