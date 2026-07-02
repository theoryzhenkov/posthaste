use std::collections::{BTreeMap, BTreeSet};

use posthaste_domain_model::{GmailLabel, MessageRecord, SystemKeyword};
use posthaste_domain_model::{MailboxId, MessageId};

use crate::{message::ImapMailboxMembershipSource, DiscoveredImapMailbox, ImapMappedHeader};

use super::group::CanonicalMessageGroup;
use super::rfc5322::Rfc5322CanonicalMessageProfile;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct GmailCanonicalMessageProfile {
    label_projector: GmailLabelProjector,
}

impl GmailCanonicalMessageProfile {
    pub(super) fn new(mailboxes: &[DiscoveredImapMailbox]) -> Self {
        Self {
            label_projector: GmailLabelProjector::new(mailboxes),
        }
    }

    pub(super) fn project_headers(&self, headers: Vec<ImapMappedHeader>) -> Vec<ImapMappedHeader> {
        let mut groups = BTreeMap::<MessageId, CanonicalMessageGroup>::new();

        for mut header in headers {
            let canonical_id = Self::canonical_message_id(&header.message);
            header.message.id = canonical_id.clone();
            header.location.message_id = canonical_id.clone();

            if let Some(labels) = header.gmail_labels.clone() {
                self.label_projector.project(&mut header, &labels);
            }

            groups
                .entry(canonical_id)
                .or_insert_with(|| CanonicalMessageGroup::new(header.message.clone()))
                .push(header);
        }

        groups
            .into_values()
            .flat_map(CanonicalMessageGroup::into_headers)
            .collect()
    }

    pub(super) fn canonical_message_id(message: &MessageRecord) -> MessageId {
        if message.id.as_str().starts_with("imap:gmail:msgid:") {
            message.id.clone()
        } else {
            Rfc5322CanonicalMessageProfile.canonical_message_id(message)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GmailLabelProjector {
    mailbox_ids_by_label: BTreeMap<GmailLabelKey, MailboxId>,
    absence_authoritative_mailbox_ids: BTreeSet<MailboxId>,
}

impl GmailLabelProjector {
    pub(super) fn new(mailboxes: &[DiscoveredImapMailbox]) -> Self {
        let mut mailbox_ids_by_label = BTreeMap::new();
        let mut absence_authoritative_mailbox_ids = BTreeSet::new();

        for mailbox in mailboxes.iter().filter(|mailbox| mailbox.selectable) {
            let system_labels = gmail_system_labels_for_mailbox(mailbox);
            if system_labels.is_empty() {
                mailbox_ids_by_label.insert(
                    GmailLabelKey::Custom(mailbox.name.clone()),
                    mailbox.id.clone(),
                );
                absence_authoritative_mailbox_ids.insert(mailbox.id.clone());
                continue;
            }

            for label in system_labels {
                if label.absence_is_authoritative() {
                    absence_authoritative_mailbox_ids.insert(mailbox.id.clone());
                }
                mailbox_ids_by_label.insert(GmailLabelKey::System(label), mailbox.id.clone());
            }
        }

        Self {
            mailbox_ids_by_label,
            absence_authoritative_mailbox_ids,
        }
    }

    fn project(&self, header: &mut ImapMappedHeader, labels: &[GmailLabel]) {
        let mut mailbox_ids = BTreeSet::from([header.location.mailbox_id.clone()]);
        let mut starred = false;

        for label in labels {
            let key = GmailLabelKey::from_label(label);
            if key == GmailLabelKey::System(GmailSystemLabel::Starred) {
                starred = true;
            }
            if let Some(mailbox_id) = self.mailbox_ids_by_label.get(&key) {
                mailbox_ids.insert(mailbox_id.clone());
            }
        }
        let provider_absent_mailbox_ids = self
            .absence_authoritative_mailbox_ids
            .difference(&mailbox_ids)
            .cloned()
            .collect();

        let flagged = SystemKeyword::Flagged.as_str();
        let mut keywords = header
            .message
            .keywords
            .iter()
            .filter(|keyword| keyword.as_str() != flagged)
            .cloned()
            .collect::<BTreeSet<_>>();
        if starred {
            keywords.insert(flagged.to_string());
        }

        header.message.mailbox_ids = mailbox_ids.into_iter().collect();
        header.message.keywords = keywords.into_iter().collect();
        header.mailbox_membership_source = ImapMailboxMembershipSource::ProviderLabels;
        header.provider_absent_mailbox_ids = provider_absent_mailbox_ids;
    }
}

fn gmail_system_labels_for_mailbox(mailbox: &DiscoveredImapMailbox) -> BTreeSet<GmailSystemLabel> {
    let mut labels = BTreeSet::new();

    if mailbox.name.eq_ignore_ascii_case("INBOX") {
        labels.insert(GmailSystemLabel::Inbox);
    }
    if gmail_mailbox_name_matches(&mailbox.name, "All Mail") {
        labels.insert(GmailSystemLabel::All);
    }
    if gmail_mailbox_name_matches(&mailbox.name, "Drafts") {
        labels.insert(GmailSystemLabel::Drafts);
    }
    if gmail_mailbox_name_matches(&mailbox.name, "Important") {
        labels.insert(GmailSystemLabel::Important);
    }
    if gmail_mailbox_name_matches(&mailbox.name, "Sent")
        || gmail_mailbox_name_matches(&mailbox.name, "Sent Mail")
    {
        labels.insert(GmailSystemLabel::Sent);
    }
    if gmail_mailbox_name_matches(&mailbox.name, "Spam") {
        labels.insert(GmailSystemLabel::Spam);
    }
    if gmail_mailbox_name_matches(&mailbox.name, "Starred") {
        labels.insert(GmailSystemLabel::Starred);
    }
    if gmail_mailbox_name_matches(&mailbox.name, "Trash") {
        labels.insert(GmailSystemLabel::Trash);
    }

    for attribute in &mailbox.attributes {
        if let Some(label) = GmailSystemLabel::from_mailbox_attribute(attribute) {
            labels.insert(label);
        }
    }

    labels
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum GmailLabelKey {
    System(GmailSystemLabel),
    Custom(String),
}

impl GmailLabelKey {
    fn from_label(label: &GmailLabel) -> Self {
        GmailSystemLabel::from_label(label.as_str())
            .map(Self::System)
            .unwrap_or_else(|| Self::Custom(label.as_str().to_string()))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum GmailSystemLabel {
    All,
    Drafts,
    Important,
    Inbox,
    Sent,
    Spam,
    Starred,
    Trash,
}

impl GmailSystemLabel {
    fn from_label(label: &str) -> Option<Self> {
        if label.eq_ignore_ascii_case("INBOX") {
            return Some(Self::Inbox);
        }

        match label.to_ascii_uppercase().as_str() {
            "\\ALL" => Some(Self::All),
            "\\DRAFTS" => Some(Self::Drafts),
            "IMPORTANT" => Some(Self::Important),
            "\\IMPORTANT" => Some(Self::Important),
            "\\INBOX" => Some(Self::Inbox),
            "\\SENT" => Some(Self::Sent),
            "\\SPAM" => Some(Self::Spam),
            "\\STARRED" => Some(Self::Starred),
            "\\TRASH" => Some(Self::Trash),
            _ => None,
        }
    }

    fn from_mailbox_attribute(attribute: &str) -> Option<Self> {
        match attribute.to_ascii_uppercase().as_str() {
            "\\ALL" => Some(Self::All),
            "\\DRAFTS" => Some(Self::Drafts),
            "\\FLAGGED" => Some(Self::Starred),
            "\\IMPORTANT" => Some(Self::Important),
            "\\INBOX" => Some(Self::Inbox),
            "\\JUNK" => Some(Self::Spam),
            "\\SENT" => Some(Self::Sent),
            "\\SPAM" => Some(Self::Spam),
            "\\TRASH" => Some(Self::Trash),
            _ => None,
        }
    }

    fn absence_is_authoritative(self) -> bool {
        !matches!(self, Self::All)
    }
}

fn gmail_mailbox_name_matches(mailbox_name: &str, expected_leaf: &str) -> bool {
    mailbox_name.eq_ignore_ascii_case(expected_leaf)
        || mailbox_name
            .rsplit_once('/')
            .is_some_and(|(_, leaf)| leaf.eq_ignore_ascii_case(expected_leaf))
}
