use posthaste_domain_model::ProviderProfile;
#[cfg(test)]
use posthaste_domain_model::{MessageId, MessageRecord, ProviderKind};

use crate::{DiscoveredImapAccount, DiscoveredImapMailbox, ImapMappedHeader};

mod gmail;
mod group;
mod rfc5322;

use gmail::GmailCanonicalMessageProfile;
use rfc5322::Rfc5322CanonicalMessageProfile;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ImapAdapterProviderProfile {
    profile: ProviderProfile,
    mailboxes: Vec<DiscoveredImapMailbox>,
}

impl ImapAdapterProviderProfile {
    pub(crate) fn from_discovery(discovery: &DiscoveredImapAccount) -> Self {
        Self {
            profile: discovery.provider_profile(),
            mailboxes: discovery.mailboxes.clone(),
        }
    }

    #[cfg(test)]
    pub(crate) fn gmail() -> Self {
        Self {
            profile: ProviderProfile::from_kind(ProviderKind::Gmail),
            mailboxes: Vec::new(),
        }
    }

    pub(crate) fn project_headers(&self, headers: Vec<ImapMappedHeader>) -> Vec<ImapMappedHeader> {
        if self.profile.imap().canonicalizes_by_gmail_message_id() {
            GmailCanonicalMessageProfile::new(&self.mailboxes).project_headers(headers)
        } else if self.profile.imap().canonicalizes_by_rfc5322_message_id() {
            Rfc5322CanonicalMessageProfile.project_headers(headers)
        } else {
            headers
        }
    }

    #[cfg(test)]
    pub(crate) fn canonical_message_id(&self, message: &MessageRecord) -> MessageId {
        if self.profile.imap().canonicalizes_by_gmail_message_id() {
            GmailCanonicalMessageProfile::canonical_message_id(message)
        } else if self.profile.imap().canonicalizes_by_rfc5322_message_id() {
            Rfc5322CanonicalMessageProfile.canonical_message_id(message)
        } else {
            message.id.clone()
        }
    }
}
