use std::collections::{BTreeMap, BTreeSet};

use posthaste_domain::{
    ImapMessageLocation, MailboxId, MessageId, MessageRecord, ProviderKind, ProviderProfile,
};

use crate::{DiscoveredImapAccount, ImapMappedHeader};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ImapAdapterProviderProfile {
    profile: ProviderProfile,
}

impl ImapAdapterProviderProfile {
    pub(crate) fn from_discovery(discovery: &DiscoveredImapAccount) -> Self {
        Self {
            profile: discovery.provider_profile(),
        }
    }

    #[cfg(test)]
    pub(crate) fn gmail() -> Self {
        Self {
            profile: ProviderProfile::from_kind(ProviderKind::Gmail),
        }
    }

    pub(crate) fn project_headers(&self, headers: Vec<ImapMappedHeader>) -> Vec<ImapMappedHeader> {
        match self.profile.kind() {
            ProviderKind::Gmail => GmailImapProviderProfile.project_headers(headers),
            ProviderKind::Generic | ProviderKind::Outlook | ProviderKind::Icloud => headers,
        }
    }

    #[cfg(test)]
    pub(crate) fn canonical_message_id(&self, message: &MessageRecord) -> MessageId {
        match self.profile.kind() {
            ProviderKind::Gmail => GmailImapProviderProfile.canonical_message_id(message),
            ProviderKind::Generic | ProviderKind::Outlook | ProviderKind::Icloud => {
                message.id.clone()
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GmailImapProviderProfile;

impl GmailImapProviderProfile {
    fn project_headers(&self, headers: Vec<ImapMappedHeader>) -> Vec<ImapMappedHeader> {
        let mut groups = BTreeMap::<MessageId, GmailCanonicalMessageGroup>::new();

        for mut header in headers {
            let canonical_id = self.canonical_message_id(&header.message);
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

    fn canonical_message_id(&self, message: &MessageRecord) -> MessageId {
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
