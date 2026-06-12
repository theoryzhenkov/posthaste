use std::collections::BTreeSet;

use posthaste_domain::{ImapMessageLocation, MailboxId, MessageRecord};

use crate::{message::ImapMailboxMembershipSource, ImapMappedHeader};

#[derive(Debug)]
pub(super) struct CanonicalMessageGroup {
    message: MessageRecord,
    mailbox_ids: BTreeSet<MailboxId>,
    keywords: BTreeSet<String>,
    locations: Vec<ImapMessageLocation>,
    provider_labels_observed: bool,
    provider_absent_mailbox_ids: Option<BTreeSet<MailboxId>>,
}

impl CanonicalMessageGroup {
    pub(super) fn new(message: MessageRecord) -> Self {
        Self {
            message,
            mailbox_ids: BTreeSet::new(),
            keywords: BTreeSet::new(),
            locations: Vec::new(),
            provider_labels_observed: false,
            provider_absent_mailbox_ids: None,
        }
    }

    pub(super) fn push(&mut self, mapped: ImapMappedHeader) {
        if mapped.mailbox_membership_source == ImapMailboxMembershipSource::ProviderLabels {
            self.provider_labels_observed = true;
            let absent = mapped
                .provider_absent_mailbox_ids
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            self.provider_absent_mailbox_ids = Some(
                self.provider_absent_mailbox_ids
                    .take()
                    .map(|current| current.intersection(&absent).cloned().collect())
                    .unwrap_or(absent),
            );
        }
        self.mailbox_ids.extend(mapped.message.mailbox_ids);
        self.keywords.extend(mapped.message.keywords);
        self.locations.push(mapped.location);
    }

    pub(super) fn into_headers(mut self) -> Vec<ImapMappedHeader> {
        self.message.mailbox_ids = self.mailbox_ids.into_iter().collect();
        self.message.keywords = self.keywords.into_iter().collect();
        let mailbox_membership_source = if self.provider_labels_observed {
            ImapMailboxMembershipSource::ProviderLabels
        } else {
            ImapMailboxMembershipSource::SelectedMailbox
        };
        let provider_absent_mailbox_ids = self
            .provider_absent_mailbox_ids
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();

        self.locations
            .into_iter()
            .map(|location| ImapMappedHeader {
                message: self.message.clone(),
                location,
                gmail_labels: None,
                mailbox_membership_source,
                provider_absent_mailbox_ids: provider_absent_mailbox_ids.clone(),
            })
            .collect()
    }
}
